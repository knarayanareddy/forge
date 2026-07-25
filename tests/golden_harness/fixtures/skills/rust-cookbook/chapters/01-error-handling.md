# Error handling

Use `thiserror` for library errors and `anyhow` for application binaries.

```rust
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ConfigError {
    #[error("missing field `{0}` in config")]
    MissingField(String),
}
```

The `?` operator propagates `Result` errors when the error type implements `From`.

**Verbatim citation anchor (SKILL-02):** `MissingField(String)` must appear in answers about config validation.
