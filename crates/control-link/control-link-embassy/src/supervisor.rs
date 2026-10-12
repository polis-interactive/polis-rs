//! One connection lifetime for RX and TX. Queues, handshake and reconnect policy
//! stay with the caller. No tasks, allocation, command storage or replay.
use crate::{Error, protocol::Message, stream};
use core::{
    cell::Cell,
    future::{Future, poll_fn},
    pin::pin,
    task::Poll,
};
use embassy_futures::select::{Either3, select3};
use embassy_time::{Duration, with_timeout};
use embedded_io_async::{Read, Write};

/// Why both connection halves were retired. Detailed I/O errors are also returned
/// to the corresponding application loop before it is cancelled.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Exit {
    Reset,
    ReceiveEnded,
    TransmitEnded,
    ReceiveFailed,
    SendFailed,
    SendTimedOut,
    SendCancelled,
}

#[derive(Debug)]
pub enum SendError<E> {
    Transport(Error<E>),
    TimedOut,
}
impl<E: core::fmt::Display> core::fmt::Display for SendError<E> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Transport(error) => error.fmt(f),
            Self::TimedOut => f.write_str("control send timed out; connection retired"),
        }
    }
}
impl<E: core::error::Error + 'static> core::error::Error for SendError<E> {}

#[derive(Default)]
struct State(Cell<Option<Exit>>);
impl State {
    fn retire(&self, exit: Exit) {
        if self.0.get().is_none() {
            self.0.set(Some(exit));
        }
    }
}

/// Application-owned connection behavior. Both loops borrow the same handler
/// concurrently: use channels or interior mutability for shared application state.
/// Futures run in one task and do not need Send. Return to end the connection.
/// Neither handshake, queues nor power/recovery policy is supplied by this trait.
pub trait ConnectionHandler<R: Read, W: Write> {
    fn receive(&self, receiver: Receiver<'_, '_, R>) -> impl Future<Output = ()>;
    fn transmit(&self, sender: Sender<'_, '_, W>) -> impl Future<Output = ()>;
}

/// Own both halves until reset, either application loop returns, or a fatal I/O
/// operation retires the connection. A send timeout includes writing and flushing.
/// The reset future is polled first, including when already ready at entry.
///
/// RX/TX application futures run in this task and must yield cooperatively. On
/// exit they and both adapters are dropped before this function returns. Dropping
/// this entire future does the same. No supervised half can escape this scope.
/// A cancelled individual send also retires the scope, even if its error is
/// ignored or the TX loop remains pending. Receive cancellation follows the
/// backend's cancellation contract; it does not itself retire the connection.
///
/// Reconnect by obtaining/reborrowing backends and calling run with a fresh pair.
/// Wait for transport readiness and establish a fresh application session. The
/// caller must invalidate/filter its old queued work; this function never keeps
/// or resends messages. Supply a reset/generation-change future for USB resets
/// that may otherwise disappear between endpoint polls. Supply pending() when
/// no external reset source exists.
pub async fn run<'a, R: Read, W: Write>(
    pair: stream::Pair<'a, R, W>,
    send_timeout: Duration,
    reset: impl Future<Output = ()>,
    handler: &impl ConnectionHandler<R, W>,
) -> Exit {
    let state = State::default();
    let (sender, receiver) = pair;
    let rx = handler.receive(Receiver {
        inner: receiver,
        state: &state,
    });
    let tx = handler.transmit(Sender {
        inner: sender,
        state: &state,
        timeout: send_timeout,
    });
    match select3(
        reset,
        watch(rx, &state, Exit::ReceiveEnded),
        watch(tx, &state, Exit::TransmitEnded),
    )
    .await
    {
        Either3::First(()) => Exit::Reset,
        Either3::Second(exit) | Either3::Third(exit) => exit,
    }
}

// Check after EACH application poll. A failing RX must not give TX one more
// chance to write before the scope notices the failure (and vice versa).
async fn watch(future: impl Future<Output = ()>, state: &State, finished: Exit) -> Exit {
    let mut future = pin!(future);
    poll_fn(|cx| {
        if let Some(exit) = state.0.get() {
            return Poll::Ready(exit);
        }
        let result = future.as_mut().poll(cx);
        if let Some(exit) = state.0.get() {
            Poll::Ready(exit)
        } else {
            result.map(|()| finished)
        }
    })
    .await
}

/// Scoped receiver. Malformed/foreign frames remain recoverable for application
/// counters/filtering; EOF (including truncated EOF) and backend errors retire
/// both halves. Retains the low-level decoder through the application handshake.
pub struct Receiver<'scope, 'a, R> {
    inner: stream::Receiver<'a, R>,
    state: &'scope State,
}
impl<R: Read> Receiver<'_, '_, R> {
    pub async fn receive<T>(
        &mut self,
        decode: impl FnOnce(Message<'_>) -> T,
    ) -> Result<T, Error<R::Error>> {
        if self.state.0.get().is_some() {
            return Err(Error::Closed);
        }
        let result = self.inner.receive(decode).await;
        if matches!(
            &result,
            Err(Error::Io(_)
                | Error::Closed
                | Error::Interrupted
                | Error::WriteZero
                | Error::Framing(polis_framing::Error::Truncated))
        ) {
            self.state.retire(Exit::ReceiveFailed);
        }
        result
    }
}

/// Scoped sender with a caller-selected deadline. Any send error, timeout, or
/// cancellation retires both halves. No operation is retried, including codec
/// errors; use the low-level split API if a different failure policy is needed.
pub struct Sender<'scope, 'a, W> {
    inner: stream::Sender<'a, W>,
    state: &'scope State,
    timeout: Duration,
}
impl<W: Write> Sender<'_, '_, W> {
    pub async fn send(&mut self, message: Message<'_>) -> Result<(), SendError<W::Error>> {
        if self.state.0.get().is_some() {
            return Err(SendError::Transport(Error::Interrupted));
        }
        let mut guard = SendGuard {
            state: self.state,
            completed: false,
        };
        let result = match with_timeout(self.timeout, self.inner.send(message)).await {
            Ok(Ok(())) => Ok(()),
            Ok(Err(error)) => {
                self.state.retire(Exit::SendFailed);
                Err(SendError::Transport(error))
            }
            Err(_) => {
                self.state.retire(Exit::SendTimedOut);
                Err(SendError::TimedOut)
            }
        };
        guard.completed = true;
        result
    }
}
struct SendGuard<'a> {
    state: &'a State,
    completed: bool,
}
impl Drop for SendGuard<'_> {
    fn drop(&mut self) {
        if !self.completed {
            self.state.retire(Exit::SendCancelled);
        }
    }
}
