use std::future::Future;

use futures::{future::LocalBoxFuture, FutureExt};

#[derive(Debug)]
pub enum ActorError {
    HandlerNotFound,
    // DispatcherNotFound,
    // StateAccessError,
    // ExecutionError(Box<dyn std::error::Error + Send + Sync>),
    CustomError(Box<dyn std::error::Error + Send + Sync>),
    SendError,
    DispatchError,
}

impl std::fmt::Display for ActorError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ActorError::HandlerNotFound => write!(f, "Handler not found"),
            // ActorError::DispatcherNotFound => write!(f, "Dispatcher not found"),
            // ActorError::StateAccessError => write!(f, "Failed to access actor state"),
            // ActorError::ExecutionError(e) => write!(f, "Execution error: {}", e),
            ActorError::CustomError(e) => write!(f, "Custom error: {}", e),
            ActorError::SendError => write!(f, "Failed to send message"),
            ActorError::DispatchError => write!(f, "Failed to dispatch message"),
        }
    }
}

impl std::error::Error for ActorError {}

pub trait AsyncFunc<'a, S, C, R, E>: Fn(&'a mut S, C) -> Self::Fut + Send + Sync
where
    S: 'static,
    C: 'static,
    R: 'static,
    E: std::error::Error + Send + Sync + 'static,
{
    type Fut: Future<Output = Result<R, E>> + Send;
}

impl<'a, F, S, C, R, E, Fut> AsyncFunc<'a, S, C, R, E> for F
where
    F: Fn(&'a mut S, C) -> Fut + Send + Sync,
    S: 'static,
    C: 'static,
    R: 'static,
    E: std::error::Error + Send + Sync + 'static,
    Fut: Future<Output = Result<R, E>> + Send,
{
    type Fut = Fut;
}

pub trait AsyncFuncImmutable<'a, S, C, R, E>: Fn(&'a S, C) -> Self::Fut + Send + Sync
where
    S: 'static,
    C: 'static,
    R: 'static,
    E: std::error::Error + Send + Sync + 'static,
{
    type Fut: Future<Output = Result<R, E>> + Send;
}

impl<'a, F, S, C, R, E, Fut> AsyncFuncImmutable<'a, S, C, R, E> for F
where
    F: Fn(&'a S, C) -> Fut + Send + Sync,
    S: 'static,
    C: 'static,
    R: 'static,
    E: std::error::Error + Send + Sync + 'static,
    Fut: Future<Output = Result<R, E>> + Send,
{
    type Fut = Fut;
}

type BoxAsyncFunc<S, C, R> =
    Box<dyn for<'a> Fn(&'a mut S, C) -> LocalBoxFuture<'a, Result<R, ActorError>> + Send + Sync>;

struct AsyncFuncHandler<S, C, R> {
    func: BoxAsyncFunc<S, C, R>,
}

impl<S: 'static, C: 'static, R: 'static> AsyncFuncHandler<S, C, R> {
    fn new<F, E>(f: F) -> Self
    where
        F: for<'a> AsyncFunc<'a, S, C, R, E> + 'static,
        E: std::error::Error + Send + Sync + 'static,
    {
        AsyncFuncHandler {
            func: Box::new(move |s, c| {
                Box::pin(f(s, c).map(|r| r.map_err(|e| ActorError::CustomError(Box::new(e)))))
            }),
        }
    }
}

type BoxAsyncFuncImmutable<S, C, R> =
    Box<dyn for<'a> Fn(&'a S, C) -> LocalBoxFuture<'a, Result<R, ActorError>> + Send + Sync>;

struct AsyncFuncHandlerImmutable<S, C, R> {
    func: BoxAsyncFuncImmutable<S, C, R>,
}

impl<S: 'static, C: 'static, R: 'static> AsyncFuncHandlerImmutable<S, C, R> {
    fn new<F, E>(f: F) -> Self
    where
        F: for<'a> AsyncFuncImmutable<'a, S, C, R, E> + 'static,
        E: std::error::Error + Send + Sync + 'static,
    {
        AsyncFuncHandlerImmutable {
            func: Box::new(move |s, c| {
                Box::pin(f(s, c).map(|r| r.map_err(|e| ActorError::CustomError(Box::new(e)))))
            }),
        }
    }
}

pub trait SyncFunc<'a, S, C, R, E>: Fn(&'a mut S, C) -> Result<R, E> + Send + Sync
where
    S: 'static,
    C: 'static,
    R: 'static,
    E: std::error::Error + Send + Sync + 'static,
{
}

impl<'a, F, S, C, R, E> SyncFunc<'a, S, C, R, E> for F
where
    F: Fn(&'a mut S, C) -> Result<R, E> + Send + Sync,
    S: 'static,
    C: 'static,
    R: 'static,
    E: std::error::Error + Send + Sync + 'static,
{
}

pub trait SyncFuncImmutable<'a, S, C, R, E>: Fn(&'a S, C) -> Result<R, E> + Send + Sync
where
    S: 'static,
    C: 'static,
    R: 'static,
    E: std::error::Error + Send + Sync + 'static,
{
}

impl<'a, F, S, C, R, E> SyncFuncImmutable<'a, S, C, R, E> for F
where
    F: Fn(&'a S, C) -> Result<R, E> + Send + Sync,
    S: 'static,
    C: 'static,
    R: 'static,
    E: std::error::Error + Send + Sync + 'static,
{
}

type BoxSyncFunc<S, C, R> =
    Box<dyn for<'a> Fn(&'a mut S, C) -> Result<R, ActorError> + Send + Sync>;

struct SyncFuncHandler<S, C, R> {
    func: BoxSyncFunc<S, C, R>,
}

impl<S: 'static, C: 'static, R: 'static> SyncFuncHandler<S, C, R> {
    fn new<F, E>(f: F) -> Self
    where
        F: for<'a> SyncFunc<'a, S, C, R, E> + 'static,
        E: std::error::Error + Send + Sync + 'static,
    {
        SyncFuncHandler {
            func: Box::new(move |s, c| f(s, c).map_err(|e| ActorError::CustomError(Box::new(e)))),
        }
    }
}

type BoxSyncFuncImmutable<S, C, R> =
    Box<dyn for<'a> Fn(&'a S, C) -> Result<R, ActorError> + Send + Sync>;

struct SyncFuncHandlerImmutable<S, C, R> {
    func: BoxSyncFuncImmutable<S, C, R>,
}

impl<S: 'static, C: 'static, R: 'static> SyncFuncHandlerImmutable<S, C, R> {
    fn new<F, E>(f: F) -> Self
    where
        F: for<'a> SyncFuncImmutable<'a, S, C, R, E> + 'static,
        E: std::error::Error + Send + Sync + 'static,
    {
        SyncFuncHandlerImmutable {
            func: Box::new(move |s, c| f(s, c).map_err(|e| ActorError::CustomError(Box::new(e)))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::executor::block_on;

    #[test]
    fn test_async_func_handler_immutable() {
        let state = 0;
        async fn handler_fn(s: &u64, c: u64) -> Result<u64, ActorError> {
            Ok(s + c)
        }

        let handler = AsyncFuncHandlerImmutable::new(handler_fn);

        let result = block_on((handler.func)(&state, 10));
        assert_eq!(result.unwrap(), 10);
    }

    #[test]
    fn test_async_func_handler() {
        let mut state = 0;
        async fn handler_fn(s: &mut u64, c: u64) -> Result<u64, ActorError> {
            *s += c;
            Ok(*s)
        }

        let handler = AsyncFuncHandler::new(handler_fn);

        let result = block_on((handler.func)(&mut state, 10));
        assert_eq!(result.unwrap(), 10);
    }

    #[test]
    fn test_sync_func_handler_immutable() {
        let state = 0;
        fn handler_fn(s: &u64, c: u64) -> Result<u64, ActorError> {
            Ok(s + c)
        }

        let handler = SyncFuncHandlerImmutable::new(handler_fn);

        let result = (handler.func)(&state, 10);
        assert_eq!(result.unwrap(), 10);
    }

    #[test]
    fn test_sync_func_handler() {
        let mut state = 0;
        fn handler_fn(s: &mut u64, c: u64) -> Result<u64, ActorError> {
            *s += c;
            Ok(*s)
        }

        let handler = SyncFuncHandler::new(handler_fn);

        let result = (handler.func)(&mut state, 10);
        assert_eq!(result.unwrap(), 10);
    }
}
