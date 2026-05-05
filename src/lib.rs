//! Mooncake Transfer Engine FFI bindings
//!
//! FFI layer for Mooncake's TransferEngine.
//! Wraps Mooncake C++ TransferEngine for zero-copy GPU memory transfer.
//!
//! Uses hardcoded peer discovery (no etcd/consul) for simple setups.

use std::ffi::{c_void, CString};
use std::ptr::NonNull;
use thiserror::Error;

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

    #[error("FFI error: {0}")]
    Ffi(String),
}

pub type Result<T> = std::result::Result<T, MooncakeError>;

/// Opaque handle to Mooncake TransferEngine
pub struct TransferEngine {
    inner: NonNull<c_void>,
}

/// Transfer request opcode
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Opcode {
    Read = 0,
    Write = 1,
}

/// Transfer status codes
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransferStatus {
    Waiting = 0,
    Pending = 1,
    Invalid = 2,
    Canceled = 3,
    Completed = 4,
    Timeout = 5,
    Failed = 6,
}

/// Buffer entry for batch memory registration
#[repr(C)]
pub struct BufferEntry {
    pub addr: *mut c_void,
    pub length: usize,
}

/// Notification message
#[repr(C)]
pub struct NotifyMsg {
    pub name: *mut libc::c_char,
    pub msg: *mut libc::c_char,
}

/// Transfer request
#[repr(C)]
pub struct TransferRequest {
    pub opcode: i32,
    pub source: *mut c_void,
    pub target_id: i32,
    pub target_offset: u64,
    pub length: u64,
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

// C struct for transfer status
#[repr(C)]
struct TransferStatusC {
    status: i32,
    transferred_bytes: u64,
}

// FFI declarations - bindgen-generated from transfer_engine_c.h
#[link(name = "transfer_engine")]
extern "C" {
    fn createTransferEngine(
        metadata_conn_string: *const libc::c_char,
        local_server_name: *const libc::c_char,
        ip_or_host_name: *const libc::c_char,
        rpc_port: u64,
        auto_discover: i32,
    ) -> *mut c_void;

    fn destroyTransferEngine(engine: *mut c_void);

    fn discoverTopology(engine: *mut c_void) -> i32;

    fn getLocalIpAndPort(engine: *mut c_void, buf_out: *mut libc::c_char, buf_len: usize) -> i32;

    fn installTransport(
        engine: *mut c_void,
        proto: *const libc::c_char,
        args: *mut c_void,
    ) -> *mut c_void;

    fn uninstallTransport(engine: *mut c_void, proto: *const libc::c_char) -> i32;

    fn openSegment(engine: *mut c_void, segment_name: *const libc::c_char) -> i32;

    fn openSegmentNoCache(engine: *mut c_void, segment_name: *const libc::c_char) -> i32;

    fn closeSegment(engine: *mut c_void, segment_id: i32) -> i32;

    fn warmupEfaSegment(engine: *mut c_void, segment_name: *const libc::c_char) -> i32;

    fn removeLocalSegment(engine: *mut c_void, segment_name: *const libc::c_char) -> i32;

    fn registerLocalMemory(
        engine: *mut c_void,
        addr: *mut c_void,
        length: usize,
        location: *const libc::c_char,
        remote_accessible: i32,
    ) -> i32;

    fn unregisterLocalMemory(engine: *mut c_void, addr: *mut c_void) -> i32;

    fn registerLocalMemoryBatch(
        engine: *mut c_void,
        buffer_list: *mut BufferEntry,
        buffer_len: usize,
        location: *const libc::c_char,
    ) -> i32;

    fn unregisterLocalMemoryBatch(
        engine: *mut c_void,
        addr_list: *mut *mut c_void,
        addr_len: usize,
    ) -> i32;

    fn allocateBatchID(engine: *mut c_void, batch_size: usize) -> u64;

    fn submitTransfer(
        engine: *mut c_void,
        batch_id: u64,
        entries: *mut TransferRequest,
        count: usize,
    ) -> i32;

    fn submitTransferWithNotify(
        engine: *mut c_void,
        batch_id: u64,
        entries: *mut TransferRequest,
        count: usize,
        notify_msg: NotifyMsg,
    ) -> i32;

    fn getNotifsFromEngine(engine: *mut c_void, size: *mut libc::c_int) -> *mut NotifyMsg;

    fn freeNotifsMsgBuf(msg: *mut NotifyMsg, size: libc::c_int) -> i32;

    fn genNotifyInEngine(engine: *mut c_void, target_id: u64, notify_msg: NotifyMsg) -> i32;

    fn getTransferStatus(
        engine: *mut c_void,
        batch_id: u64,
        task_id: usize,
        status: *mut TransferStatusC,
    ) -> i32;

    fn freeBatchID(engine: *mut c_void, batch_id: u64) -> i32;

    fn syncSegmentCache(engine: *mut c_void) -> i32;
}
