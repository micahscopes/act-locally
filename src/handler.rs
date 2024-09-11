use std::{any::Any, future::Future};

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

pub(crate) type BoxedAny = Box<dyn Any + Send>;

trait MessageHandler<S>: Send + Sync {
    fn handle<'a>(
        &'a self,
        state: &'a mut S,
        message: BoxedAny,
    ) -> LocalBoxFuture<'a, Result<BoxedAny, ActorError>>;
}

impl<S, C, R> MessageHandler<S> for AsyncFuncHandler<S, C, R>
where
    S: 'static,
    C: 'static + Send,
    R: 'static + Send,
{
    fn handle<'a>(
        &'a self,
        state: &'a mut S,
        message: BoxedAny,
    ) -> LocalBoxFuture<'a, Result<BoxedAny, ActorError>> {
        Box::pin(async move {
            let params = message.downcast::<C>().map_err(|_| {
                println!("Downcast error in handle");
                ActorError::CustomError(Box::new(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "Invalid message type",
                )))
            })?;
            let result = (self.func)(state, *params).await?;
            Ok(Box::new(result) as BoxedAny)
        })
    }
}

impl<S, C, R> MessageHandler<S> for AsyncFuncHandlerImmutable<S, C, R>
where
    S: 'static,
    C: 'static + Send,
    R: 'static + Send,
{
    fn handle<'a>(
        &'a self,
        state: &'a mut S,
        message: BoxedAny,
    ) -> LocalBoxFuture<'a, Result<BoxedAny, ActorError>> {
        Box::pin(async move {
            let params = message.downcast::<C>().map_err(|_| {
                println!("Downcast error in handle");
                ActorError::CustomError(Box::new(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "Invalid message type",
                )))
            })?;
            let result = (self.func)(state, *params).await?;
            Ok(Box::new(result) as BoxedAny)
        })
    }
}

impl<S, C, R> MessageHandler<S> for SyncFuncHandler<S, C, R>
where
    S: 'static,
    C: 'static + Send,
    R: 'static + Send,
{
    fn handle<'a>(
        &'a self,
        state: &'a mut S,
        message: BoxedAny,
    ) -> LocalBoxFuture<'a, Result<BoxedAny, ActorError>> {
        Box::pin(async move {
            let params = message.downcast::<C>().map_err(|_| {
                println!("Downcast error in handle");
                ActorError::CustomError(Box::new(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "Invalid message type",
                )))
            })?;
            let result = (self.func)(state, *params)?;
            Ok(Box::new(result) as BoxedAny)
        })
    }
}

impl<S, C, R> MessageHandler<S> for SyncFuncHandlerImmutable<S, C, R>
where
    S: 'static,
    C: 'static + Send,
    R: 'static + Send,
{
    fn handle<'a>(
        &'a self,
        state: &'a mut S,
        message: BoxedAny,
    ) -> LocalBoxFuture<'a, Result<BoxedAny, ActorError>> {
        Box::pin(async move {
            let params = message.downcast::<C>().map_err(|_| {
                println!("Downcast error in handle");
                ActorError::CustomError(Box::new(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "Invalid message type",
                )))
            })?;
            let result = (self.func)(state, *params)?;
            Ok(Box::new(result) as BoxedAny)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::executor::block_on;

    #[test]
    fn test_async_func_handler_immutable_message_handler() {
        let mut state = 0u64;
        async fn handler_fn(s: &u64, c: u64) -> Result<u64, ActorError> {
            Ok(s + c)
        }

        let handler = AsyncFuncHandlerImmutable::new(handler_fn);

        let message = Box::new(10u64);
        let result = block_on(handler.handle(&mut state, message));

        let result = result.unwrap().downcast::<u64>().unwrap();
        assert_eq!(*result, 10);
    }

    #[test]
    fn test_async_func_handler_message_handler() {
        let mut state = 0;
        async fn handler_fn(s: &mut u64, c: u64) -> Result<u64, ActorError> {
            *s += c;
            Ok(*s)
        }

        let handler = AsyncFuncHandler::new(handler_fn);

        let message = Box::new(10u64);
        let result = block_on(handler.handle(&mut state, message));
        let result = result.unwrap().downcast::<u64>().unwrap();
        assert_eq!(*result, 10);
    }

    #[test]
    fn test_sync_func_handler_immutable_message_handler() {
        let mut state = 0;
        fn handler_fn(s: &u64, c: u64) -> Result<u64, ActorError> {
            Ok(s + c)
        }

        let handler = SyncFuncHandlerImmutable::new(handler_fn);

        let message = Box::new(10u64);
        let result = block_on(handler.handle(&mut state, message));
        let result = result.unwrap().downcast::<u64>().unwrap();
        assert_eq!(*result, 10);
    }

    #[test]
    fn test_sync_func_handler_message_handler() {
        let mut state = 0;
        fn handler_fn(s: &mut u64, c: u64) -> Result<u64, ActorError> {
            *s += c;
            Ok(*s)
        }

        let handler = SyncFuncHandler::new(handler_fn);

        let message = Box::new(10u64);
        let result = block_on(handler.handle(&mut state, message));
        let result = result.unwrap().downcast::<u64>().unwrap();
        assert_eq!(*result, 10);
    }

    #[test]
    fn test_store_handlers_in_hashmap() {
        use std::collections::HashMap;

        let mut state = 0u64;

        async fn async_handler_mut(s: &mut u64, c: u64) -> Result<u64, ActorError> {
            *s += c;
            Ok(*s)
        }

        async fn async_handler_immut(s: &u64, c: u64) -> Result<u64, ActorError> {
            Ok(*s)
        }

        fn sync_handler_mut(s: &mut u64, c: u64) -> Result<u64, ActorError> {
            *s += c;
            Ok(*s)
        }

        fn sync_handler_immut(s: &u64, c: u64) -> Result<u64, ActorError> {
            Ok(*s)
        }

        let mut handlers: HashMap<&str, Box<dyn MessageHandler<u64>>> = HashMap::new();

        handlers.insert(
            "async_mut",
            Box::new(AsyncFuncHandler::new(async_handler_mut)),
        );
        handlers.insert(
            "async_immut",
            Box::new(AsyncFuncHandlerImmutable::new(async_handler_immut)),
        );
        handlers.insert("sync_mut", Box::new(SyncFuncHandler::new(sync_handler_mut)));
        handlers.insert(
            "sync_immut",
            Box::new(SyncFuncHandlerImmutable::new(sync_handler_immut)),
        );

        let message = Box::new(10u64);

        let result = block_on(handlers["async_mut"].handle(&mut state, message.clone()));
        let result = result.unwrap().downcast::<u64>().unwrap();
        assert_eq!(*result, 10);

        let result = block_on(handlers["async_immut"].handle(&mut state, message.clone()));
        let result = result.unwrap().downcast::<u64>().unwrap();
        assert_eq!(*result, 10);

        let result = block_on(handlers["sync_mut"].handle(&mut state, message.clone()));
        let result = result.unwrap().downcast::<u64>().unwrap();
        assert_eq!(*result, 20);

        let result = block_on(handlers["sync_immut"].handle(&mut state, message.clone()));
        let result = result.unwrap().downcast::<u64>().unwrap();
        assert_eq!(*result, 20);
    }

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
