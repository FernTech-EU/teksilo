<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# Contributing to Teksilo

Thank you for your interest in contributing to Teksilo! This document provides guidelines and information for contributors.

## Code of Conduct

Please be respectful and constructive in all interactions. We aim to maintain a welcoming environment for everyone.

## Authorship and review

Teksilo is built under the following eight rules. They apply to your contributions too.

1. Direct human communication is written by humans. PR messages, issues, posts, replies: no AI drafting, no AI polish. Common decency.

2. Documentation may be drafted by AI; every line is reviewed by a human. API examples must compile against the current API. Claims are checked, not skimmed.

3. Code, including tests, may be written by AI; every line is reviewed by a human. "Reviewed" means the reviewer understands the change well enough to defend it without the AI in the room. Vibe coding is forbidden. Plausible-looking code is not reviewed code.

4. Architecture and public API are human. AI implements within them; it does not design them. The load-bearing surface is specified by a human: the `Widget` trait, `Signal`/`Prop`, the event model, anything downstream apps depend on.

5. Authors and reviewers, both human, are the voluntary bottleneck. Final responsibility rests with them, not the AI. They may use any tool to help, AI included; what is missed lands on them regardless. They take their time; high-speed AI output is not a reason for high-speed work.

6. The human who signs the work owns it, AI or not. Provenance is not disclosed in commits or PR text.

7. No AI has ever been condemned by judges. Only humans and companies have. Stay sharp.

## How to Contribute

### Reporting Issues

- Check existing issues before creating a new one
- Provide a clear description of the problem
- Include steps to reproduce, expected behavior, and actual behavior
- Mention your environment (OS, Rust version, etc.)

### Suggesting Features

- Open an issue describing the feature and its use case
- Explain why this would be valuable for Teksilo users
- Be open to discussion about alternative approaches

### Submitting Code

1. Fork the repository
2. Create a feature branch from `main`
3. Make your changes
4. Ensure your code follows the project's style
5. Test your changes
6. Submit a pull request

## Developer Certificate of Origin

This project uses the [Developer Certificate of Origin (DCO)](DCO.md).

By contributing to this repository, you agree to the DCO. You **must sign off your commits** to indicate your agreement:

```bash
git commit -s -m "Your commit message"
```

This adds a `Signed-off-by: Your Name <your.email@example.com>` line to your commit, certifying that you wrote or have the right to submit the code under the project's license (MPL-2.0).

### Setting up automatic sign-off

You can configure Git to always set your identity for your commits for this repository:

```bash
git config user.name "Your Name"
git config user.email "your.email@example.com"
```

Then use `git commit -s` for each commit, or create a Git alias:

```bash
git config --global alias.cs "commit -s"
```

### What if I forgot to sign off?

You can amend your last commit:

```bash
git commit --amend -s
```

For multiple commits, you may need to rebase:

```bash
git rebase --signoff HEAD~N
```

(Replace `N` with the number of commits to sign off)

## License

By contributing to Teksilo, you agree that your contributions will be licensed under the [Mozilla Public License 2.0](LICENSE).

## Questions?

If you have questions about contributing, feel free to open an issue for discussion.
