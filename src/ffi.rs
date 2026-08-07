//! The C boundary: `transfer_engine_c.h` and the types that cross it.
//!
//! Nothing here interprets anything. The declarations mirror the header one for
//! one, and the `#[repr(C)]` types are laid out for the engine, not for Rust.
//! Everything that decides something lives above this, in `lib.rs`.

use std::ffi::c_void;

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

// C struct for transfer status
#[repr(C)]
pub(crate) struct TransferStatusC {
    pub(crate) status: i32,
    pub(crate) transferred_bytes: u64,
}

// FFI declarations - bindgen-generated from transfer_engine_c.h
#[link(name = "transfer_engine")]
extern "C" {
    pub(crate) fn createTransferEngine(
        metadata_conn_string: *const libc::c_char,
        local_server_name: *const libc::c_char,
        ip_or_host_name: *const libc::c_char,
        rpc_port: u64,
        auto_discover: i32,
    ) -> *mut c_void;

    pub(crate) fn destroyTransferEngine(engine: *mut c_void);

    pub(crate) fn discoverTopology(engine: *mut c_void) -> i32;

    pub(crate) fn getLocalIpAndPort(engine: *mut c_void, buf_out: *mut libc::c_char, buf_len: usize) -> i32;

    pub(crate) fn installTransport(
        engine: *mut c_void,
        proto: *const libc::c_char,
        args: *mut *mut c_void,
    ) -> *mut c_void;

    pub(crate) fn uninstallTransport(engine: *mut c_void, proto: *const libc::c_char) -> i32;

    pub(crate) fn openSegment(engine: *mut c_void, segment_name: *const libc::c_char) -> i32;

    pub(crate) fn openSegmentNoCache(engine: *mut c_void, segment_name: *const libc::c_char) -> i32;

    pub(crate) fn closeSegment(engine: *mut c_void, segment_id: i32) -> i32;

    pub(crate) fn warmupEfaSegment(engine: *mut c_void, segment_name: *const libc::c_char) -> i32;

    pub(crate) fn removeLocalSegment(engine: *mut c_void, segment_name: *const libc::c_char) -> i32;

    pub(crate) fn registerLocalMemory(
        engine: *mut c_void,
        addr: *mut c_void,
        length: usize,
        location: *const libc::c_char,
        remote_accessible: i32,
    ) -> i32;

    pub(crate) fn unregisterLocalMemory(engine: *mut c_void, addr: *mut c_void) -> i32;

    pub(crate) fn registerLocalMemoryBatch(
        engine: *mut c_void,
        buffer_list: *mut BufferEntry,
        buffer_len: usize,
        location: *const libc::c_char,
    ) -> i32;

    pub(crate) fn unregisterLocalMemoryBatch(
        engine: *mut c_void,
        addr_list: *mut *mut c_void,
        addr_len: usize,
    ) -> i32;

    pub(crate) fn allocateBatchID(engine: *mut c_void, batch_size: usize) -> u64;

    pub(crate) fn submitTransfer(
        engine: *mut c_void,
        batch_id: u64,
        entries: *mut TransferRequest,
        count: usize,
    ) -> i32;

    pub(crate) fn submitTransferWithNotify(
        engine: *mut c_void,
        batch_id: u64,
        entries: *mut TransferRequest,
        count: usize,
        notify_msg: NotifyMsg,
    ) -> i32;

    pub(crate) fn getNotifsFromEngine(engine: *mut c_void, size: *mut libc::c_int) -> *mut NotifyMsg;

    pub(crate) fn freeNotifsMsgBuf(msg: *mut NotifyMsg, size: libc::c_int) -> i32;

    pub(crate) fn genNotifyInEngine(engine: *mut c_void, target_id: u64, notify_msg: NotifyMsg) -> i32;

    pub(crate) fn getTransferStatus(
        engine: *mut c_void,
        batch_id: u64,
        task_id: usize,
        status: *mut TransferStatusC,
    ) -> i32;

    pub(crate) fn freeBatchID(engine: *mut c_void, batch_id: u64) -> i32;

    pub(crate) fn syncSegmentCache(engine: *mut c_void) -> i32;
}
