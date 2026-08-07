//! Mooncake Transfer Engine bindings
//!
//! Zero-copy GPU memory transfer over Mooncake's TransferEngine, with hardcoded
//! peer discovery (no etcd/consul) for simple setups.
//!
//! Three layers, in one direction:
//!
//! - `ffi` declares the C boundary and nothing else.
//! - [`topology`] is pure Rust with no boundary at all.
//! - This module is the safe API: it owns the engine handle, turns return codes
//!   into [`MooncakeError`], and is the only place the two meet.

use std::ffi::{c_void, CString};
use std::ptr::NonNull;
use thiserror::Error;

mod ffi;
pub mod topology;

use crate::ffi::*;
pub use crate::ffi::{BufferEntry, NotifyMsg, Opcode, TransferRequest, TransferStatus};
pub use crate::topology::NicPriorityMatrix;

/// Errors from Mooncake FFI operations
#[derive(Error, Debug)]
pub enum MooncakeError {
    #[error("Failed to create transfer engine: {0}")]
    EngineCreation(String),

    #[error("Failed to register memory: {0}")]
    MemoryRegistration(String),

    #[error("Failed to install transport: {0}")]
    TransportInstall(String),

    #[error("Failed to open segment: {0}")]
    SegmentOpen(String),

    #[error("Transfer failed: {0}")]
    Transfer(String),

    #[error("Invalid string (contains null bytes): {0}")]
    InvalidString(String),

    /// Self-describing, so a caller that got the name from a flag or a config
    /// key can prefix it with that name and read as one sentence.
    #[error("{0}")]
    InvalidPeerName(String),

    #[error("FFI error: {0}")]
    Ffi(String),
}

pub type Result<T> = std::result::Result<T, MooncakeError>;


/// Reject a name the peer will not be able to dial.
///
/// `P2PHANDSHAKE` is bidirectional: the name an engine is created with is what
/// the peer dials back to set up the RDMA endpoint, so it has to resolve from
/// the other host. Both ways of getting it wrong survive startup and surface
/// much later as errors that read like a NIC fault — `received packet mismatch`
/// with an empty `peer.local_nic_path`, or `Peer nic not found in that server:
/// :PORT@ibp0` — so they are caught here instead.
///
/// The empty string is rejected because an engine named `""` starts, registers
/// and plans normally, and fails only once a peer tries to resolve a host from
/// `:PORT`. A name carrying a port is rejected because the engine appends its
/// own, making `host:a:b`.
pub fn validate_peer_name(name: &str) -> Result<()> {
    if name.trim().is_empty() {
        return Err(MooncakeError::InvalidPeerName(
            "peer name is empty; it must be an address the peer can dial, such as the output of \
             `hostname -i`"
                .to_string(),
        ));
    }

    if name.contains(':') {
        return Err(MooncakeError::InvalidPeerName(format!(
            "peer name {name:?} must not include a port; mooncake appends its own"
        )));
    }

    Ok(())
}

/// Opaque handle to Mooncake TransferEngine
pub struct TransferEngine {
    inner: NonNull<c_void>,
}


impl TransferEngine {
    /// Create new TransferEngine with minimal metadata config
    ///
    /// For hardcoded setup without etcd/consul, use empty metadata_conn_string
    /// and set auto_discover=0
    ///
    /// # Arguments
    /// * `metadata_conn_string` - Empty string "" for no metadata service
    /// * `local_server_name` - Unique name for this node (e.g., "prefill-1")
    /// * `ip_or_hostname` - Local IP to bind for RPC
    /// * `rpc_port` - Port for RPC communication
    pub fn new(
        metadata_conn_string: &str,
        local_server_name: &str,
        ip_or_hostname: &str,
        rpc_port: u64,
    ) -> Result<Self> {
        let metadata_c = CString::new(metadata_conn_string)
            .map_err(|e| MooncakeError::InvalidString(e.to_string()))?;
        let server_name_c = CString::new(local_server_name)
            .map_err(|e| MooncakeError::InvalidString(e.to_string()))?;
        let ip_c = CString::new(ip_or_hostname)
            .map_err(|e| MooncakeError::InvalidString(e.to_string()))?;

        let ptr = unsafe {
            createTransferEngine(
                metadata_c.as_ptr(),
                server_name_c.as_ptr(),
                ip_c.as_ptr(),
                rpc_port,
                0, // auto_discover = false (hardcoded topology)
            )
        };

        NonNull::new(ptr)
            .map(|inner| Self { inner })
            .ok_or_else(|| MooncakeError::EngineCreation("Failed to create engine".to_string()))
    }

    /// Install a transport protocol (e.g., "tcp", "rdma", "nvmeof")
    ///
    /// The engine picks NICs from the topology it discovered. On a host with
    /// several HCAs that can pair a buffer with a NIC under a different PCIe
    /// switch than the GPU holding it; use [`Self::install_transport_with_topology`]
    /// to say which NIC serves which memory location.
    pub fn install_transport(&self, proto: &str) -> Result<()> {
        let proto_c =
            CString::new(proto).map_err(|e| MooncakeError::InvalidString(e.to_string()))?;

        let transport = unsafe {
            installTransport(self.inner.as_ptr(), proto_c.as_ptr(), std::ptr::null_mut())
        };

        if transport.is_null() {
            Err(MooncakeError::TransportInstall(format!(
                "Failed to install transport '{}',",
                proto
            )))
        } else {
            Ok(())
        }
    }

    /// Install a transport protocol with an explicit NIC priority matrix.
    ///
    /// `nic_priority_matrix` is JSON mapping a memory location to two lists of
    /// HCA names, preferred and fallback:
    ///
    /// ```json
    /// {"cuda:0": [["ibp4"], []], "cpu:0": [["ibp4"], []]}
    /// ```
    ///
    /// The keys are the `location` strings passed to
    /// [`Self::register_memory`] and [`Self::register_memory_batch`], so a
    /// buffer registered as `cuda:0` is served by `ibp4`.
    ///
    /// The matrix **replaces** the discovered topology rather than extending
    /// it, so it must name every location this process registers memory under.
    /// A location missing from it has no NIC to fall back to.
    pub fn install_transport_with_topology(
        &self,
        proto: &str,
        nic_priority_matrix: &str,
    ) -> Result<()> {
        let proto_c =
            CString::new(proto).map_err(|e| MooncakeError::InvalidString(e.to_string()))?;
        let matrix_c = CString::new(nic_priority_matrix)
            .map_err(|e| MooncakeError::InvalidString(e.to_string()))?;

        // The engine reads `args[0]` as a NUL-terminated matrix string and
        // ignores the rest, so a one-element array is enough.
        let mut args: [*mut c_void; 1] = [matrix_c.as_ptr() as *mut c_void];

        let transport = unsafe {
            installTransport(self.inner.as_ptr(), proto_c.as_ptr(), args.as_mut_ptr())
        };

        if transport.is_null() {
            Err(MooncakeError::TransportInstall(format!(
                "Failed to install transport '{}',",
                proto
            )))
        } else {
            Ok(())
        }
    }

    /// Install a transport protocol with a built NIC priority matrix.
    ///
    /// Same as [`Self::install_transport_with_topology`], but the matrix is
    /// constructed rather than hand-written. See [`NicPriorityMatrix`].
    ///
    /// An empty matrix is rejected: it parses, installs, and leaves every
    /// location without a NIC, which fails at the first transfer rather than
    /// here.
    pub fn install_transport_with_matrix(
        &self,
        proto: &str,
        matrix: &NicPriorityMatrix,
    ) -> Result<()> {
        if matrix.is_empty() {
            return Err(MooncakeError::TransportInstall(
                "NIC priority matrix is empty; it replaces the discovered topology, so every \
                 location this process registers memory under must appear in it"
                    .to_string(),
            ));
        }

        self.install_transport_with_topology(proto, &matrix.to_json())
    }

    /// Uninstall a transport protocol
    pub fn uninstall_transport(&self, proto: &str) -> Result<()> {
        let proto_c =
            CString::new(proto).map_err(|e| MooncakeError::InvalidString(e.to_string()))?;

        let rc = unsafe { uninstallTransport(self.inner.as_ptr(), proto_c.as_ptr()) };

        if rc == 0 {
            Ok(())
        } else {
            Err(MooncakeError::TransportInstall(format!(
                "uninstallTransport returned {}",
                rc
            )))
        }
    }

    /// Discover topology (for auto-discover setups)
    pub fn discover_topology(&self) -> Result<()> {
        let rc = unsafe { discoverTopology(self.inner.as_ptr()) };
        if rc == 0 {
            Ok(())
        } else {
            Err(MooncakeError::Ffi(format!(
                "discoverTopology returned {}",
                rc
            )))
        }
    }

    /// Register GPU memory buffer with Mooncake
    ///
    /// `location` names the memory to the engine's topology and is how a NIC
    /// gets pinned to it; it must be a key of the matrix passed to
    /// [`Self::install_transport_with_matrix`], since that matrix replaces the
    /// discovered topology rather than extending it.
    ///
    /// Two failure modes here are worth knowing because neither is reported the
    /// way it reads, and neither can be checked from this side:
    ///
    /// - **Oversized registrations are silently truncated, not rejected.** A
    ///   `length` above the device's `max_mr_size` is clamped to it, with a
    ///   warning on the RDMA context path and none at all on the transport
    ///   path. Registration then succeeds while the tail of the buffer is not
    ///   registered, and only a transfer touching that tail fails. The limit is
    ///   the device's, lowered further by `MC_MAX_MR_SIZE` if set, so there is
    ///   no constant to compare against here.
    ///
    /// - **`addr` must be the base of its CUDA allocation, not merely
    ///   aligned.** Without `nvidia-peermem` the engine registers device memory
    ///   through `ibv_reg_dmabuf_mr`, and asks CUDA for a dmabuf handle
    ///   covering the whole range containing `addr`, starting at `addr`. Passing
    ///   a suballocated pointer makes that range overrun its allocation and
    ///   fails with `Failed to retrieve dmabuf ... invalid argument`, which
    ///   names neither the pointer nor the cause.
    ///
    /// # Safety
    /// Caller must ensure `addr` is valid GPU memory and remains valid
    /// until unregistered
    pub unsafe fn register_memory(
        &self,
        addr: *mut c_void,
        length: usize,
        location: &str,
    ) -> Result<()> {
        let location_c =
            CString::new(location).map_err(|e| MooncakeError::InvalidString(e.to_string()))?;

        let rc = registerLocalMemory(self.inner.as_ptr(), addr, length, location_c.as_ptr(), 1);

        if rc == 0 {
            Ok(())
        } else {
            Err(MooncakeError::MemoryRegistration(format!(
                "registerLocalMemory returned {}",
                rc
            )))
        }
    }

    /// Unregister GPU memory buffer
    ///
    /// # Safety
    /// Caller must ensure `addr` was previously registered
    pub unsafe fn unregister_memory(&self, addr: *mut c_void) -> Result<()> {
        let rc = unregisterLocalMemory(self.inner.as_ptr(), addr);

        if rc == 0 {
            Ok(())
        } else {
            Err(MooncakeError::MemoryRegistration(format!(
                "unregisterLocalMemory returned {}",
                rc
            )))
        }
    }

    /// Open a segment (peer) by name
    ///
    /// For hardcoded setup, segment_name is the remote server's name
    /// (e.g., "decode-1" or "prefill-1")
    pub fn open_segment(&self, segment_name: &str) -> Result<i32> {
        let name_c =
            CString::new(segment_name).map_err(|e| MooncakeError::InvalidString(e.to_string()))?;

        let segment_id = unsafe { openSegment(self.inner.as_ptr(), name_c.as_ptr()) };

        if segment_id < 0 {
            Err(MooncakeError::SegmentOpen(format!(
                "Failed to open segment '{}' (returned {})",
                segment_name, segment_id
            )))
        } else {
            Ok(segment_id)
        }
    }

    /// Open a segment (peer) by name without cache
    pub fn open_segment_no_cache(&self, segment_name: &str) -> Result<i32> {
        let name_c =
            CString::new(segment_name).map_err(|e| MooncakeError::InvalidString(e.to_string()))?;

        let segment_id = unsafe { openSegmentNoCache(self.inner.as_ptr(), name_c.as_ptr()) };

        if segment_id < 0 {
            Err(MooncakeError::SegmentOpen(format!(
                "Failed to open segment '{}' (returned {})",
                segment_name, segment_id
            )))
        } else {
            Ok(segment_id)
        }
    }

    /// Close a segment
    pub fn close_segment(&self, segment_id: i32) -> Result<()> {
        let rc = unsafe { closeSegment(self.inner.as_ptr(), segment_id) };

        if rc == 0 {
            Ok(())
        } else {
            Err(MooncakeError::SegmentOpen(format!(
                "closeSegment returned {}",
                rc
            )))
        }
    }

    /// Warmup EFA endpoints to a segment (eliminates first-batch fi_av_insert stall)
    pub fn warmup_efa_segment(&self, segment_name: &str) -> Result<()> {
        let name_c =
            CString::new(segment_name).map_err(|e| MooncakeError::InvalidString(e.to_string()))?;

        let rc = unsafe { warmupEfaSegment(self.inner.as_ptr(), name_c.as_ptr()) };

        if rc == 0 {
            Ok(())
        } else {
            Err(MooncakeError::Ffi(format!(
                "warmupEfaSegment returned {}",
                rc
            )))
        }
    }

    /// Remove a local segment by name
    pub fn remove_local_segment(&self, segment_name: &str) -> Result<()> {
        let name_c =
            CString::new(segment_name).map_err(|e| MooncakeError::InvalidString(e.to_string()))?;

        let rc = unsafe { removeLocalSegment(self.inner.as_ptr(), name_c.as_ptr()) };

        if rc == 0 {
            Ok(())
        } else {
            Err(MooncakeError::Ffi(format!(
                "removeLocalSegment returned {}",
                rc
            )))
        }
    }

    /// Register a batch of memory buffers
    ///
    /// # Safety
    /// Caller must ensure all buffers are valid and remain valid until unregistered
    pub unsafe fn register_memory_batch(
        &self,
        buffers: &mut [BufferEntry],
        location: &str,
    ) -> Result<()> {
        let location_c =
            CString::new(location).map_err(|e| MooncakeError::InvalidString(e.to_string()))?;

        let rc = registerLocalMemoryBatch(
            self.inner.as_ptr(),
            buffers.as_mut_ptr(),
            buffers.len(),
            location_c.as_ptr(),
        );

        if rc == 0 {
            Ok(())
        } else {
            Err(MooncakeError::MemoryRegistration(format!(
                "registerLocalMemoryBatch returned {}",
                rc
            )))
        }
    }

    /// Unregister a batch of memory buffers
    ///
    /// # Safety
    /// Caller must ensure all addresses were previously registered
    pub unsafe fn unregister_memory_batch(&self, addrs: &mut [*mut c_void]) -> Result<()> {
        let rc = unregisterLocalMemoryBatch(self.inner.as_ptr(), addrs.as_mut_ptr(), addrs.len());

        if rc == 0 {
            Ok(())
        } else {
            Err(MooncakeError::MemoryRegistration(format!(
                "unregisterLocalMemoryBatch returned {}",
                rc
            )))
        }
    }

    /// Allocate a batch ID for submitting transfer requests
    pub fn allocate_batch_id(&self, batch_size: usize) -> Result<u64> {
        let batch_id = unsafe { allocateBatchID(self.inner.as_ptr(), batch_size) };

        if batch_id == u64::MAX {
            Err(MooncakeError::Transfer(
                "Failed to allocate batch ID".to_string(),
            ))
        } else {
            Ok(batch_id)
        }
    }

    /// Submit a batch of transfer requests
    ///
    /// # Arguments
    /// * `batch_id` - Batch ID from allocate_batch_id
    /// * `requests` - Array of transfer requests
    pub fn submit_transfer(&self, batch_id: u64, requests: &mut [TransferRequest]) -> Result<()> {
        let rc = unsafe {
            submitTransfer(
                self.inner.as_ptr(),
                batch_id,
                requests.as_mut_ptr(),
                requests.len(),
            )
        };

        if rc == 0 {
            Ok(())
        } else {
            Err(MooncakeError::Transfer(format!(
                "submitTransfer returned {}",
                rc
            )))
        }
    }

    /// Submit a batch of transfer requests with notification
    ///
    /// # Arguments
    /// * `batch_id` - Batch ID from allocate_batch_id
    /// * `requests` - Array of transfer requests
    /// * `notify_name` - Notification name
    /// * `notify_msg` - Notification message
    pub fn submit_transfer_with_notify(
        &self,
        batch_id: u64,
        requests: &mut [TransferRequest],
        notify_name: &str,
        notify_msg: &str,
    ) -> Result<()> {
        let name_c =
            CString::new(notify_name).map_err(|e| MooncakeError::InvalidString(e.to_string()))?;
        let msg_c =
            CString::new(notify_msg).map_err(|e| MooncakeError::InvalidString(e.to_string()))?;

        let notify = NotifyMsg {
            name: name_c.as_ptr() as *mut libc::c_char,
            msg: msg_c.as_ptr() as *mut libc::c_char,
        };

        let rc = unsafe {
            submitTransferWithNotify(
                self.inner.as_ptr(),
                batch_id,
                requests.as_mut_ptr(),
                requests.len(),
                notify,
            )
        };

        if rc == 0 {
            Ok(())
        } else {
            Err(MooncakeError::Transfer(format!(
                "submitTransferWithNotify returned {}",
                rc
            )))
        }
    }

    /// Get notifications from engine
    ///
    /// Returns a vector of (name, message) pairs
    pub fn get_notifs(&self) -> Result<Vec<(String, String)>> {
        let mut size: libc::c_int = 0;
        let ptr = unsafe { getNotifsFromEngine(self.inner.as_ptr(), &mut size) };

        if ptr.is_null() {
            return Ok(Vec::new());
        }

        let mut result = Vec::with_capacity(size as usize);
        unsafe {
            for i in 0..size {
                let msg = &*ptr.offset(i as isize);
                let name = if msg.name.is_null() {
                    String::new()
                } else {
                    std::ffi::CStr::from_ptr(msg.name)
                        .to_string_lossy()
                        .into_owned()
                };
                let message = if msg.msg.is_null() {
                    String::new()
                } else {
                    std::ffi::CStr::from_ptr(msg.msg)
                        .to_string_lossy()
                        .into_owned()
                };
                result.push((name, message));
            }
            freeNotifsMsgBuf(ptr, size);
        }

        Ok(result)
    }

    /// Generate a notification in the engine
    pub fn gen_notify(&self, target_id: u64, name: &str, msg: &str) -> Result<()> {
        let name_c = CString::new(name).map_err(|e| MooncakeError::InvalidString(e.to_string()))?;
        let msg_c = CString::new(msg).map_err(|e| MooncakeError::InvalidString(e.to_string()))?;

        let notify = NotifyMsg {
            name: name_c.as_ptr() as *mut libc::c_char,
            msg: msg_c.as_ptr() as *mut libc::c_char,
        };

        let rc = unsafe { genNotifyInEngine(self.inner.as_ptr(), target_id, notify) };

        if rc == 0 {
            Ok(())
        } else {
            Err(MooncakeError::Transfer(format!(
                "genNotifyInEngine returned {}",
                rc
            )))
        }
    }

    /// Get status of a transfer
    ///
    /// Returns (status_code, transferred_bytes)
    pub fn get_transfer_status(&self, batch_id: u64, task_id: usize) -> Result<(i32, u64)> {
        let mut status: TransferStatusC = unsafe { std::mem::zeroed() };

        let rc = unsafe { getTransferStatus(self.inner.as_ptr(), batch_id, task_id, &mut status) };

        if rc == 0 {
            Ok((status.status, status.transferred_bytes))
        } else {
            Err(MooncakeError::Transfer(format!(
                "getTransferStatus returned {}",
                rc
            )))
        }
    }

    /// Free a batch ID
    pub fn free_batch_id(&self, batch_id: u64) -> Result<()> {
        let rc = unsafe { freeBatchID(self.inner.as_ptr(), batch_id) };

        if rc == 0 {
            Ok(())
        } else {
            Err(MooncakeError::Transfer(format!(
                "freeBatchID returned {}",
                rc
            )))
        }
    }

    /// Wait for batch completion (blocking)
    pub fn wait_for_batch(&self, batch_id: u64, task_count: usize) -> Result<()> {
        loop {
            let mut all_completed = true;
            let mut any_failed = false;

            for task_id in 0..task_count {
                let (status, _) = self.get_transfer_status(batch_id, task_id)?;
                match status {
                    4 => {} // Completed
                    5 => return Err(MooncakeError::Transfer("Timeout".to_string())),
                    6 => any_failed = true,
                    _ => all_completed = false,
                }
            }

            if any_failed {
                return Err(MooncakeError::Transfer("Transfer failed".to_string()));
            }

            if all_completed {
                return Ok(());
            }

            std::thread::yield_now();
        }
    }

    /// Submit one batch and block until every request in it has landed.
    ///
    /// This is [`Self::allocate_batch_id`], [`Self::submit_transfer`],
    /// [`Self::wait_for_batch`] and [`Self::free_batch_id`] in the one order
    /// that does not leak. The id is freed on every path, including a failed
    /// submit and a failed wait: the engine's pool of ids is finite, so a run
    /// of transfers that returned early would eventually be unable to start
    /// one, and the failure then names the wrong operation.
    ///
    /// `requests` is taken as a single batch and is not split. The engine
    /// allocates per batch id, so callers moving a large plan should chunk it
    /// themselves; there is no fixed limit on the RDMA path, only the size of
    /// the allocation this implies.
    pub fn submit_and_wait(&self, requests: &mut [TransferRequest]) -> Result<()> {
        if requests.is_empty() {
            return Ok(());
        }

        let count = requests.len();
        let batch_id = self.allocate_batch_id(count)?;

        let result = self
            .submit_transfer(batch_id, requests)
            .and_then(|()| self.wait_for_batch(batch_id, count));

        let freed = self.free_batch_id(batch_id);

        result?;
        freed
    }

    /// Transfer data from remote to local (blocking helper)
    ///
    /// # Arguments
    /// * `segment_id` - Remote segment ID
    /// * `remote_offset` - Offset in remote memory
    /// * `local_addr` - Local GPU destination
    /// * `length` - Bytes to transfer
    pub fn transfer_from_remote(
        &self,
        segment_id: i32,
        remote_offset: u64,
        local_addr: *mut c_void,
        length: u64,
    ) -> Result<()> {
        let batch_id = self.allocate_batch_id(1)?;

        let mut requests = [TransferRequest {
            opcode: Opcode::Read as i32,
            source: local_addr,
            target_id: segment_id,
            target_offset: remote_offset,
            length,
        }];

        self.submit_transfer(batch_id, &mut requests)?;
        self.wait_for_batch(batch_id, 1)?;
        self.free_batch_id(batch_id)?;

        Ok(())
    }

    /// Transfer data from local to remote (blocking helper)
    ///
    /// # Arguments
    /// * `segment_id` - Remote segment ID
    /// * `remote_offset` - Offset in remote memory
    /// * `local_addr` - Local GPU source
    /// * `length` - Bytes to transfer
    pub fn transfer_to_remote(
        &self,
        segment_id: i32,
        remote_offset: u64,
        local_addr: *mut c_void,
        length: u64,
    ) -> Result<()> {
        let batch_id = self.allocate_batch_id(1)?;

        let mut requests = [TransferRequest {
            opcode: Opcode::Write as i32,
            source: local_addr,
            target_id: segment_id,
            target_offset: remote_offset,
            length,
        }];

        self.submit_transfer(batch_id, &mut requests)?;
        self.wait_for_batch(batch_id, 1)?;
        self.free_batch_id(batch_id)?;

        Ok(())
    }
}

impl TransferEngine {
    /// Get the local IP and port assigned by Mooncake
    pub fn get_local_addr(&self) -> Result<String> {
        let mut buf = vec![0u8; 256];
        let ret = unsafe {
            getLocalIpAndPort(
                self.inner.as_ptr(),
                buf.as_mut_ptr() as *mut libc::c_char,
                buf.len(),
            )
        };
        if ret != 0 {
            return Err(MooncakeError::Ffi(
                "Failed to get local address".to_string(),
            ));
        }
        let addr = std::str::from_utf8(&buf)
            .map_err(|e| MooncakeError::Ffi(format!("Invalid UTF-8: {}", e)))?
            .trim_end_matches('\0')
            .to_string();
        Ok(addr)
    }

    /// Sync segment cache to ensure local segment is visible to peers
    pub fn sync_segment_cache(&self) -> Result<()> {
        let ret = unsafe { syncSegmentCache(self.inner.as_ptr()) };
        if ret != 0 {
            return Err(MooncakeError::Ffi(format!(
                "syncSegmentCache returned {}",
                ret
            )));
        }
        Ok(())
    }
}

impl Drop for TransferEngine {
    fn drop(&mut self) {
        unsafe {
            destroyTransferEngine(self.inner.as_ptr());
        }
    }
}

// Safety: TransferEngine is thread-safe (Mooncake handles synchronization)
unsafe impl Send for TransferEngine {}
unsafe impl Sync for TransferEngine {}


#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn peer_name_must_be_dialable() {
        assert!(validate_peer_name("10.0.1.170").is_ok());
        assert!(validate_peer_name("").is_err());
        assert!(validate_peer_name("   ").is_err());
        assert!(validate_peer_name("10.0.1.170:12345").is_err());
    }
}
