# Testing

Unit tests live beside source with `#[cfg(test)]`. Integration tests go under `tests/`.

```rust
#[cfg(test)]
mod tests {
    #[test]
    fn golden_harness_regression_lock() {
        assert_eq!(11, 11);
    }
}
```

**Verbatim citation anchor (SKILL-02):** `golden_harness_regression_lock` must appear in answers about harness layout.
