use std::{any::Any, future::Future};

use futures::{future::LocalBoxFuture, FutureExt};

#[derive(Debug)]
pub enum ActorError {
    HandlerNotFound,
    CustomError(Box<dyn std::error::Error + Send + Sync>),
    SendError,
    DispatchError,
}

impl std::fmt::Display for ActorError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ActorError::HandlerNotFound => write!(f, "Handler not found"),
            ActorError::CustomError(e) => write!(f, "Custom error: {}", e),
            ActorError::SendError => write!(f, "Failed to send message"),
            ActorError::DispatchError => write!(f, "Failed to dispatch message"),
        }
    }
}

impl std::error::Error for ActorError {}

pub trait AsyncMutatingFunc<'a, S, C, R, E>: Fn(&'a mut S, C) -> Self::Fut + Send + Sync
where
    S: 'static,
    C: 'static,
    R: 'static,
    E: std::error::Error + Send + Sync + 'static,
{
    type Fut: Future<Output = Result<R, E>> + Send;
}

impl<'a, F, S, C, R, E, Fut> AsyncMutatingFunc<'a, S, C, R, E> for F
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

pub trait AsyncFunc<'a, S, C, R, E>: Fn(&'a S, C) -> Self::Fut + Send + Sync
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
    F: Fn(&'a S, C) -> Fut + Send + Sync,
    S: 'static,
    C: 'static,
    R: 'static,
    E: std::error::Error + Send + Sync + 'static,
    Fut: Future<Output = Result<R, E>> + Send,
{
    type Fut = Fut;
}

type BoxAsyncMutatingFunc<S, C, R> =
    Box<dyn for<'a> Fn(&'a mut S, C) -> LocalBoxFuture<'a, Result<R, ActorError>> + Send + Sync>;

struct AsyncMutatingHandler<S, C, R> {
    func: BoxAsyncMutatingFunc<S, C, R>,
}

impl<S: 'static, C: 'static, R: 'static> AsyncMutatingHandler<S, C, R> {
    fn new<F, E>(f: F) -> Self
    where
        F: for<'a> AsyncMutatingFunc<'a, S, C, R, E> + 'static,
        E: std::error::Error + Send + Sync + 'static,
    {
        AsyncMutatingHandler {
            func: Box::new(move |s, c| {
                Box::pin(f(s, c).map(|r| r.map_err(|e| ActorError::CustomError(Box::new(e)))))
            }),
        }
    }
}

type BoxAsyncFunc<S, C, R> =
    Box<dyn for<'a> Fn(&'a S, C) -> LocalBoxFuture<'a, Result<R, ActorError>> + Send + Sync>;

struct AsyncHandler<S, C, R> {
    func: BoxAsyncFunc<S, C, R>,
}

impl<S: 'static, C: 'static, R: 'static> AsyncHandler<S, C, R> {
    fn new<F, E>(f: F) -> Self
    where
        F: for<'a> AsyncFunc<'a, S, C, R, E> + 'static,
        E: std::error::Error + Send + Sync + 'static,
    {
        AsyncHandler {
            func: Box::new(move |s, c| {
                Box::pin(f(s, c).map(|r| r.map_err(|e| ActorError::CustomError(Box::new(e)))))
            }),
        }
    }
}

pub trait SyncMutatingFunc<'a, S, C, R, E>: Fn(&'a mut S, C) -> Result<R, E> + Send + Sync
where
    S: 'static,
    C: 'static,
    R: 'static,
    E: std::error::Error + Send + Sync + 'static,
{
}

impl<'a, F, S, C, R, E> SyncMutatingFunc<'a, S, C, R, E> for F
where
    F: Fn(&'a mut S, C) -> Result<R, E> + Send + Sync,
    S: 'static,
    C: 'static,
    R: 'static,
    E: std::error::Error + Send + Sync + 'static,
{
}

pub trait SyncFunc<'a, S, C, R, E>: Fn(&'a S, C) -> Result<R, E> + Send + Sync
where
    S: 'static,
    C: 'static,
    R: 'static,
    E: std::error::Error + Send + Sync + 'static,
{
}

impl<'a, F, S, C, R, E> SyncFunc<'a, S, C, R, E> for F
where
    F: Fn(&'a S, C) -> Result<R, E> + Send + Sync,
    S: 'static,
    C: 'static,
    R: 'static,
    E: std::error::Error + Send + Sync + 'static,
{
}

type BoxSyncMutatingFunc<S, C, R> =
    Box<dyn for<'a> Fn(&'a mut S, C) -> Result<R, ActorError> + Send + Sync>;

struct SyncMutatingHandler<S, C, R> {
    func: BoxSyncMutatingFunc<S, C, R>,
}

impl<S: 'static, C: 'static, R: 'static> SyncMutatingHandler<S, C, R> {
    fn new<F, E>(f: F) -> Self
    where
        F: for<'a> SyncMutatingFunc<'a, S, C, R, E> + 'static,
        E: std::error::Error + Send + Sync + 'static,
    {
        SyncMutatingHandler {
            func: Box::new(move |s, c| f(s, c).map_err(|e| ActorError::CustomError(Box::new(e)))),
        }
    }
}

type BoxSyncFunc<S, C, R> = Box<dyn for<'a> Fn(&'a S, C) -> Result<R, ActorError> + Send + Sync>;

struct SyncHandler<S, C, R> {
    func: BoxSyncFunc<S, C, R>,
}

impl<S: 'static, C: 'static, R: 'static> SyncHandler<S, C, R> {
    fn new<F, E>(f: F) -> Self
    where
        F: for<'a> SyncFunc<'a, S, C, R, E> + 'static,
        E: std::error::Error + Send + Sync + 'static,
    {
        SyncHandler {
            func: Box::new(move |s, c| f(s, c).map_err(|e| ActorError::CustomError(Box::new(e)))),
        }
    }
}

pub(crate) type BoxedAny = Box<dyn Any + Send>;

// Define a common Message trait with an `as_any` method
pub trait Message: Any + Send {
    fn as_any(&self) -> &dyn Any;
}

// Blanket implementation for all types that satisfy the trait bounds
impl<T: Any + Send> Message for T {
    fn as_any(&self) -> &dyn Any {
        self
    }
}

// Define the AsyncMessageHandler trait
trait AsyncMessageHandler<S>: Send + Sync {
    fn handle<'a>(
        &'a self,
        state: &'a mut S,
        message: &dyn Message,
    ) -> LocalBoxFuture<'a, Result<Box<dyn Any + Send>, ActorError>>;
}

// Define the SyncMessageHandler trait
trait SyncMessageHandler<S>: Send + Sync {
    fn handle_sync<'a>(
        &'a self,
        state: &'a mut S,
        message: &dyn Message,
    ) -> Result<Box<dyn Any + Send>, ActorError>;
}

// Implement AsyncMessageHandler for AsyncMutatingHandler
impl<S, C, R> AsyncMessageHandler<S> for AsyncMutatingHandler<S, C, R>
where
    S: 'static,
    C: 'static + Send + Clone,
    R: 'static + Send + Any,
{
    fn handle<'a>(
        &'a self,
        state: &'a mut S,
        message: &dyn Message,
    ) -> LocalBoxFuture<'a, Result<Box<dyn Any + Send>, ActorError>> {
        if let Some(params) = message.as_any().downcast_ref::<C>() {
            let result = (self.func)(state, params.clone());
            Box::pin(async move {
                result.await.map(|r| Box::new(r) as Box<dyn Any + Send>)
            })
        } else {
            Box::pin(async { Err(ActorError::DispatchError) })
        }
    }
}

// Implement AsyncMessageHandler for AsyncHandler
impl<S, C, R> AsyncMessageHandler<S> for AsyncHandler<S, C, R>
where
    S: 'static,
    C: 'static + Send + Clone,
    R: 'static + Send + Any,
{
    fn handle<'a>(
        &'a self,
        state: &'a mut S,
        message: &dyn Message,
    ) -> LocalBoxFuture<'a, Result<Box<dyn Any + Send>, ActorError>> {
        if let Some(params) = message.as_any().downcast_ref::<C>() {
            let result = (self.func)(state, params.clone());
            Box::pin(async move {
                result.await.map(|r| Box::new(r) as Box<dyn Any + Send>)
            })
        } else {
            Box::pin(async { Err(ActorError::DispatchError) })
        }
    }
}

// Implement both AsyncMessageHandler and SyncMessageHandler for SyncMutatingHandler
impl<S, C, R> AsyncMessageHandler<S> for SyncMutatingHandler<S, C, R>
where
    S: 'static,
    C: 'static + Send + Clone,
    R: 'static + Send + Any,
{
    fn handle<'a>(
        &'a self,
        state: &'a mut S,
        message: &dyn Message,
    ) -> LocalBoxFuture<'a, Result<Box<dyn Any + Send>, ActorError>> {
        if let Some(params) = message.as_any().downcast_ref::<C>() {
            let result = (self.func)(state, params.clone());
            Box::pin(async move {
                result.map(|r| Box::new(r) as Box<dyn Any + Send>)
            })
        } else {
            Box::pin(async { Err(ActorError::DispatchError) })
        }
    }
}

impl<S, C, R> SyncMessageHandler<S> for SyncMutatingHandler<S, C, R>
where
    S: 'static,
    C: 'static + Send + Clone,
    R: 'static + Send + Any,
{
    fn handle_sync<'a>(
        &'a self,
        state: &'a mut S,
        message: &dyn Message,
    ) -> Result<Box<dyn Any + Send>, ActorError> {
        if let Some(params) = message.as_any().downcast_ref::<C>() {
            (self.func)(state, params.clone()).map(|r| Box::new(r) as Box<dyn Any + Send>)
        } else {
            Err(ActorError::DispatchError)
        }
    }
}

// Implement both AsyncMessageHandler and SyncMessageHandler for SyncHandler
impl<S, C, R> AsyncMessageHandler<S> for SyncHandler<S, C, R>
where
    S: 'static,
    C: 'static + Send + Clone,
    R: 'static + Send + Any,
{
    fn handle<'a>(
        &'a self,
        state: &'a mut S,
        message: &dyn Message,
    ) -> LocalBoxFuture<'a, Result<Box<dyn Any + Send>, ActorError>> {
        if let Some(params) = message.as_any().downcast_ref::<C>() {
            let result = (self.func)(state, params.clone());
            Box::pin(async move {
                result.map(|r| Box::new(r) as Box<dyn Any + Send>)
            })
        } else {
            Box::pin(async { Err(ActorError::DispatchError) })
        }
    }
}

impl<S, C, R> SyncMessageHandler<S> for SyncHandler<S, C, R>
where
    S: 'static,
    C: 'static + Send + Clone,
    R: 'static + Send + Any,
{
    fn handle_sync<'a>(
        &'a self,
        state: &'a mut S,
        message: &dyn Message,
    ) -> Result<Box<dyn Any + Send>, ActorError> {
        if let Some(params) = message.as_any().downcast_ref::<C>() {
            (self.func)(state, params.clone()).map(|r| Box::new(r) as Box<dyn Any + Send>)
        } else {
            Err(ActorError::DispatchError)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::executor::block_on;

    #[test]
    fn test_async_non_mutating_message_handler() {
        let mut state = 0u64;
        async fn handler_fn(s: &u64, c: u64) -> Result<u64, ActorError> {
            Ok(s + c)
        }

        let handler = AsyncHandler::new(handler_fn);

        let message = Box::new(10u64);
        let result = block_on(handler.handle(&mut state, message.as_ref()));

        let result = result.unwrap().downcast::<u64>().unwrap();
        assert_eq!(*result, 10);
    }

    #[test]
    fn test_async_mutating_message_handler() {
        let mut state = 0;
        async fn handler_fn(s: &mut u64, c: u64) -> Result<u64, ActorError> {
            *s += c;
            Ok(*s)
        }

        let handler = AsyncMutatingHandler::new(handler_fn);

        let message = Box::new(10u64);
        let result = block_on(handler.handle(&mut state, message.as_ref()));
        let result = result.unwrap().downcast::<u64>().unwrap();
        assert_eq!(*result, 10);
    }

    #[test]
    fn test_sync_non_mutating_message_handler() {
        let mut state = 0;
        fn handler_fn(s: &u64, c: u64) -> Result<u64, ActorError> {
            Ok(s + c)
        }

        let handler = SyncHandler::new(handler_fn);

        let message = Box::new(10u64);
        let result = block_on(handler.handle(&mut state, message.as_ref()));
        let result = result.unwrap().downcast::<u64>().unwrap();
        assert_eq!(*result, 10);
    }

    #[test]
    fn test_sync_mutating_message_handler() {
        let mut state = 0;
        fn handler_fn(s: &mut u64, c: u64) -> Result<u64, ActorError> {
            *s += c;
            Ok(*s)
        }

        let handler = SyncMutatingHandler::new(handler_fn);

        let message = Box::new(10u64);
        let result = block_on(handler.handle(&mut state, message.as_ref()));
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

        let mut handlers: HashMap<&str, Box<dyn AsyncMessageHandler<u64>>> = HashMap::new();

        handlers.insert(
            "async_mut",
            Box::new(AsyncMutatingHandler::new(async_handler_mut)),
        );
        handlers.insert(
            "async_immut",
            Box::new(AsyncHandler::new(async_handler_immut)),
        );
        handlers.insert(
            "sync_mut",
            Box::new(SyncMutatingHandler::new(sync_handler_mut)),
        );
        handlers.insert("sync_immut", Box::new(SyncHandler::new(sync_handler_immut)));

        let message = Box::new(10u64);

        let result = block_on(handlers["async_mut"].handle(&mut state, message.as_ref()));
        let result = result.unwrap().downcast::<u64>().unwrap();
        assert_eq!(*result, 10);

        let result = block_on(handlers["async_immut"].handle(&mut state, message.as_ref()));
        let result = result.unwrap().downcast::<u64>().unwrap();
        assert_eq!(*result, 10);

        let result = block_on(handlers["sync_mut"].handle(&mut state, message.as_ref()));
        let result = result.unwrap().downcast::<u64>().unwrap();
        assert_eq!(*result, 20);

        let result = block_on(handlers["sync_immut"].handle(&mut state, message.as_ref()));
        let result = result.unwrap().downcast::<u64>().unwrap();
        assert_eq!(*result, 20);
    }
    #[test]
    fn test_store_sync_handlers_in_hashmap() {
        use std::collections::HashMap;

        let mut state = 0u64;

        fn sync_handler_mut(s: &mut u64, c: u64) -> Result<u64, ActorError> {
            *s += c;
            Ok(*s)
        }

        fn sync_handler_immut(s: &u64, c: u64) -> Result<u64, ActorError> {
            Ok(s + c)
        }

        let mut handlers: HashMap<&str, Box<dyn SyncMessageHandler<u64>>> = HashMap::new();

        handlers.insert(
            "sync_mut",
            Box::new(SyncMutatingHandler::new(sync_handler_mut)),
        );
        handlers.insert(
            "sync_immut",
            Box::new(SyncHandler::new(sync_handler_immut)),
        );

        let message = Box::new(10u64);

        let result = handlers["sync_mut"].handle_sync(&mut state, message.as_ref());
        let result = result.unwrap().downcast::<u64>().unwrap();
        assert_eq!(*result, 10);

        let result = handlers["sync_immut"].handle_sync(&mut state, message.as_ref());
        let result = result.unwrap().downcast::<u64>().unwrap();
        assert_eq!(*result, 20);
    }

    #[test]
    fn test_async_non_mutating_func_wrapper() {
        let state = 0;
        async fn handler_fn(s: &u64, c: u64) -> Result<u64, ActorError> {
            Ok(s + c)
        }

        let handler = AsyncHandler::new(handler_fn);

        let result = block_on((handler.func)(&state, 10));
        assert_eq!(result.unwrap(), 10);
    }

    #[test]
    fn test_async_mutating_func_wrapper() {
        let mut state = 0;
        async fn handler_fn(s: &mut u64, c: u64) -> Result<u64, ActorError> {
            *s += c;
            Ok(*s)
        }

        let handler = AsyncMutatingHandler::new(handler_fn);

        let result = block_on((handler.func)(&mut state, 10));
        assert_eq!(result.unwrap(), 10);
    }

    #[test]
    fn test_sync_non_mutating_func_wrapper() {
        let state = 0;
        fn handler_fn(s: &u64, c: u64) -> Result<u64, ActorError> {
            Ok(s + c)
        }

        let handler = SyncHandler::new(handler_fn);
        
        let result = (handler.func)(&state, 10);
        assert_eq!(result.unwrap(), 10);
    }

    #[test]
    fn test_sync_mutating_func_wrapper() {
        let mut state = 0;
        fn handler_fn(s: &mut u64, c: u64) -> Result<u64, ActorError> {
            *s += c;
            Ok(*s)
        }

        let handler = SyncMutatingHandler::new(handler_fn);

        let result = (handler.func)(&mut state, 10);
        assert_eq!(result.unwrap(), 10);
    }
}
