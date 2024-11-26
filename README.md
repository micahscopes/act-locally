# act-locally

Simple single-threaded async actors with ordinary functions as handlers and plainly typed messages.

## Why another actor framework?
### **Actor frameworks in Rust are an epidemic**

In that spirit, `act-locally` was created with two primary goals:

1. **Simplicity in handler definition and message types**: Use ordinary Rust functions as handlers, reducing boilerplate and making your code more intuitive.  Use ordinary types as message/return types.

2. **Support for `!Send`/`!Sync` state**: Leverage thread-local async executors so that shared state doesn't need to be thread-safe.

These features allow for more natural Rust code within an actor model, and enable use cases that many other actor frameworks don't support out of the box.

## Features

- Both synchronous and asynchronous message handlers
- Supports mutating and non-mutating handlers
- Flexible dispatcher system for message routing
- Type-safe message passing
- Built on `smol` for async runtime
- Integrates with `tracing` for observability

## TODO
- Support closures as handlers
- Documentation and examples
- Offer more control over order-execution when combining mutating and non-mutating handlers (currently a write-preferring read/write lock is used)
- Stream integration
- Benchmarks and optimization

## License

Licensed under MIT. See [LICENSE](LICENSE) for details.
