# Repository Guidance

## Development environment

All Cargo operations must be run inside the Nix development environment. Use
`nix develop` before running `cargo` commands.

## Commits

- Use Conventional Commits syntax: `type(scope): description`.
- Keep the subject concise, imperative, and focused on the user-visible change.
- When a commit includes specification work, include a commit body that summarizes specifications under the applicable `Implemented`, `Changed`, and `Removed` headings.
- Mention relevant validation performed when it materially helps reviewers assess the commit.
- Do not include unrelated working-tree changes in a commit.
