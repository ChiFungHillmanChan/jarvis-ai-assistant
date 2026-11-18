//! Each chat wire pinned through `AiRouter::send_one` (P07): the request it
//! sends, how it reads a recorded reply, what the chat window and the voice
//! get, the tool round and its approval, the usage row and the error text.
//! The replies are fixtures written from each provider's documented stream
//! format and served from a loopback address: nothing leaves this Mac.

use std::cell::RefCell;

thread_local! {
    /// Where [`api_base`] sends this thread's requests instead of the
    /// provider's own address.
    pub(super) static API_BASE: RefCell<Option<String>> = const { RefCell::new(None) };
}
