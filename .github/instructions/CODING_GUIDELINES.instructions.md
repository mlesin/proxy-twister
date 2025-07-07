---
applyTo: "**/*.rs"
---

Coding standards, domain knowledge, and preferences that AI should follow.

- In formatting, prefer to include the variable name in the format string for clarity, e.g., `println!("Variable: {variable_name}")` instead of `println!("Variable: {}", variable_name)`.
- Use `format!` instead of `&format!` for string formatting where possible.
- Check `context7` mcp for the latest coding standards and preferences.

- To run tests use `cargo test`.
