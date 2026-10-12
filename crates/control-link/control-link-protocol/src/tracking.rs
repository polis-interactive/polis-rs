use core::{fmt, num::NonZeroU32, num::NonZeroU64};

use crate::{Key, Message};

/// A matched reply or an unsolicited event from the current session.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Incoming<'a> {
    Accepted { key: Key },
    Response { key: Key, body: &'a [u8] },
    Event { body: &'a [u8] },
}

/// Bounded outstanding requests for one session. No payloads, timers or retries.
///
/// The connection owner supplies a fresh, nonzero session nonce on reconnect,
/// and sends requests in reservation order. Multiple slots allow keepalives
/// during a pending operation. Use a separate tracker for each direction if
/// both peers originate requests. Never reuse a nonce when replacing a tracker.
pub struct Requests<const N: usize> {
    session: NonZeroU64,
    next_id: Option<NonZeroU32>,
    pending: [Option<NonZeroU32>; N],
}

impl<const N: usize> Requests<N> {
    pub const fn new(session: NonZeroU64) -> Self {
        Self {
            session,
            next_id: NonZeroU32::new(1),
            pending: [None; N],
        }
    }

    pub const fn session(&self) -> NonZeroU64 {
        self.session
    }

    pub fn pending_count(&self) -> usize {
        self.pending.iter().filter(|slot| slot.is_some()).count()
    }

    /// Reserve a unique key before enqueueing/sending a request.
    /// IDs are never reused, even if the caller cancels before transmission.
    pub fn reserve(&mut self) -> Result<Key, StateError> {
        let id = self.next_id.ok_or(StateError::IdsExhausted)?;
        let slot = self
            .pending
            .iter_mut()
            .find(|slot| slot.is_none())
            .ok_or(StateError::Full)?;
        *slot = Some(id);
        self.next_id = id.get().checked_add(1).and_then(NonZeroU32::new);
        Ok(Key {
            session: self.session,
            id,
        })
    }

    /// Retire a timed-out/cancelled request. Late replies become UnknownRequest.
    /// This does not cancel work at the peer: after any possible transmission,
    /// report an uncertain outcome to the application, never automatically retry.
    pub fn cancel(&mut self, key: Key) -> Result<(), StateError> {
        *self.slot(key)? = None;
        Ok(())
    }

    /// Match replies without blocking unsolicited events or other requests.
    /// Accepted retains its slot; only a terminal Response releases it.
    pub fn receive<'a>(&mut self, message: Message<'a>) -> Result<Incoming<'a>, StateError> {
        if message.session() != self.session {
            return Err(StateError::WrongSession);
        }
        match message {
            Message::Accepted { key } => {
                self.slot(key)?;
                Ok(Incoming::Accepted { key })
            }
            Message::Response { key, body } => {
                *self.slot(key)? = None;
                Ok(Incoming::Response { key, body })
            }
            Message::Event { body, .. } => Ok(Incoming::Event { body }),
            Message::Request { .. } => Err(StateError::UnexpectedRequest),
        }
    }

    fn slot(&mut self, key: Key) -> Result<&mut Option<NonZeroU32>, StateError> {
        if key.session != self.session {
            return Err(StateError::WrongSession);
        }
        self.pending
            .iter_mut()
            .find(|slot| **slot == Some(key.id))
            .ok_or(StateError::UnknownRequest)
    }
}

/// Reject duplicate and out-of-order requests before invoking a business handler.
///
/// Create after the application's handshake accepts a fresh session. Retain this
/// gate for that session, including presence timeouts; rebuilding it with the same
/// nonce would erase replay protection. This is ordering protection, not auth.
pub struct RequestGate {
    session: NonZeroU64,
    last_id: u32,
}

impl RequestGate {
    pub const fn new(session: NonZeroU64) -> Self {
        Self {
            session,
            last_id: 0,
        }
    }

    /// Record acceptance before starting the operation. IDs may skip values.
    pub fn accept(&mut self, key: Key) -> Result<(), StateError> {
        if key.session != self.session {
            return Err(StateError::WrongSession);
        }
        if key.id.get() <= self.last_id {
            return Err(StateError::DuplicateRequest);
        }
        self.last_id = key.id.get();
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StateError {
    Full,
    IdsExhausted,
    WrongSession,
    UnknownRequest,
    UnexpectedRequest,
    DuplicateRequest,
}

impl fmt::Display for StateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Full => "all request slots are occupied",
            Self::IdsExhausted => "request IDs exhausted; a fresh session is required",
            Self::WrongSession => "message belongs to another session",
            Self::UnknownRequest => "reply or cancellation has no outstanding request",
            Self::UnexpectedRequest => "route incoming requests to the business handler",
            Self::DuplicateRequest => "request ID has already been accepted or superseded",
        })
    }
}

impl core::error::Error for StateError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exhaustion_never_wraps_or_reuses_ids() {
        let mut requests = Requests::<1>::new(NonZeroU64::new(1).unwrap());
        requests.next_id = NonZeroU32::new(u32::MAX);
        let key = requests.reserve().unwrap();
        assert_eq!(key.id.get(), u32::MAX);
        requests.cancel(key).unwrap();
        assert_eq!(requests.reserve(), Err(StateError::IdsExhausted));
    }
}
