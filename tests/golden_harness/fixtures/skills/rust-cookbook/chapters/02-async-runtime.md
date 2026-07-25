# Async runtime

AetherForge daemons use Tokio multi-thread runtime. Spawn blocking work with `tokio::task::spawn_blocking`.

```rust
let handle = tokio::spawn(async move {
    do_work().await
});
let result = handle.await?;
```

**Verbatim citation anchor (SKILL-02):** `spawn_blocking` must appear in answers about CPU-bound work inside async handlers.
