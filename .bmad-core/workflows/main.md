# BMAD Workflow for Graphon

## Overview

This document describes the BMAD (Build More, Architect Dreams) development workflow
adapted for the Graphon email indexing, sorting, and cleaning project.

## Agents

| Agent | File | Responsibilities |
|---|---|---|
| Architect (Larry) | `.bmad-core/agents/architect.md` | System design, ADRs, trait interfaces |
| Developer (Dev) | `.bmad-core/agents/developer.md` | Implementation, tests, PRs |
| Product Manager (PM) | `.bmad-core/agents/pm.md` | Stories, acceptance criteria, backlog |
| Scrum Master (SM) | `.bmad-core/agents/scrum-master.md` | Sprint planning, DoD, blockers |

## Templates

| Template | File | Use When |
|---|---|---|
| User Story | `.bmad-core/templates/story.md` | Defining any new feature or bug fix |
| ADR | `.bmad-core/templates/adr.md` | Making a significant arch decision |

## Development Workflow

```
1. IDEATE       → PM writes a user story using story.md template
                  Stored in: .bmad-core/data/stories/

2. DESIGN       → Architect reviews story, writes technical notes,
                  creates ADR if needed
                  ADRs stored in: .bmad-core/data/adrs/

3. READY        → SM confirms Definition of Ready is met
                  Story moves to sprint backlog

4. IMPLEMENT    → Developer implements following developer.md conventions
                  Branch: feature/S[N]-[story-id]-[short-title]

5. REVIEW       → PR opened, CI must pass:
                    cargo fmt --all
                    cargo clippy --workspace -- -D warnings
                    cargo test --workspace

6. DONE         → Merged, SM marks story Done
```

## Data Directory Structure

```
.bmad-core/data/
├── stories/           # User stories (story.md instances)
│   └── S1-01-*.md
├── sprints/           # Sprint plans
│   └── sprint-1.md
├── adrs/              # Architecture Decision Records
│   └── adr-001-*.md
└── epics/             # Epic definitions
    └── epic-*.md
```

## Graphon-Specific BMAD Rules

### For any new Gmail Integration trait / adapter:
1. Must pass all mock tests.
2. Under no circumstances should secrets be stored in raw code (use `credentials.json` or Environment Variables).

### For any new HTTP / CLI endpoint:
1. Must include Prometheus metrics where applicable.
2. Must have tracing spans for observability.

## Quick Start Commands

```bash
# Build
cargo build --release

# Test
cargo test --workspace

# Lint
cargo clippy --workspace -- -D warnings

# Format
cargo fmt --all
```

## Resources

- Project context: `.bmad-core/bmad-project.md`
- BMAD docs: https://bmadcode.com/
- Graphon README: `README.md`
