# gitstream Structured Planning Approach

## Core Principle
Use this structured approach for non-trivial software engineering efforts in the gitstream project (refactoring, major features, architectural changes).

## When to Use
**Use for**: Non-trivial features (5+ files, 2+ weeks), moderate refactoring, feature enhancements with multiple components, system integrations

**Don't use for**: Bug fixes, very small features (< 3 files), documentation-only, configuration-only changes

## Directory Structure

```
plans/<project-name>/
├── SUMMARY.md            # Overview, status tracking, decisions, risks, metrics (single source of truth)
├── 01-analysis/
│   ├── problem.md       # Current state + problems + open questions
│   └── impact.md        # File-by-file impact + dependency graph
├── 02-design/
│   ├── architecture.md  # Target architecture + interfaces + data flow
│   └── testing.md       # Test strategy + rollout approach
└── 03-implementation/   # step-NN-description.md files
```

## Dependency-Based Prioritization

Order work based on what unblocks other work:

```
P0: Core interfaces (everything depends on these)
P1: Shared utilities (many components use these)
P2: Individual components (depend on P0/P1)
P3: Cleanup/polish (depends on P2 being stable)
```

**Priority formula**: `Priority = (Items it unblocks) × (Risk level) × (Centrality to architecture)`

## Resuming a Project (Read Order)

When starting a new session on an existing project:

1. **Read `SUMMARY.md` first** — check current phase, current step, last updated date
2. **If in Analysis**: read `problem.md` and `impact.md`
3. **If in Design**: read `architecture.md` and `testing.md`
4. **If in Implementation**: read the current step document (the one marked `→ IN PROGRESS` or the first unchecked `- [ ]` step in SUMMARY.md)
5. **Check for deviations**: look at the Implementation Log in SUMMARY.md for any notes from prior sessions

Do NOT re-read all documents every session. SUMMARY.md tells you where you are; only read the phase documents relevant to the current work.

---

## Phased Process

### Phase 1: Analysis (FORMAL REVIEW REQUIRED)

1. Create directory structure:
   ```bash
   mkdir -p plans/<project-name>/{01-analysis,02-design,03-implementation}
   ```

2. Write analysis documents (complete ALL before review):
   - `01-analysis/problem.md`
   - `01-analysis/impact.md`
   - `SUMMARY.md`

3. Verify analysis completion:
   - [ ] Scope is bounded — can list all files/components affected
   - [ ] Problems are cataloged with file:line references
   - [ ] Dependencies are mapped
   - [ ] Risks are identified
   - [ ] No known unknowns remain

4. **FORMAL REVIEW REQUIRED** — Do not create design or implementation documents yet

### Phase 2: Design (After Analysis Review)

1. Create design documents:
   - `02-design/architecture.md`
   - `02-design/testing.md`
2. Update `SUMMARY.md` status tracking
3. **FORMAL REVIEW REQUIRED** — Do not proceed to Phase 3

### Phase 3: Implementation (After Design Review)

1. Create one implementation step document at a time (`03-implementation/step-NN-*.md`)
2. **FORMAL REVIEW REQUIRED** after each step's planning
3. Execute step after review approval
4. **After each step completes**, update `SUMMARY.md`:
   - Mark the step checkbox `[x]`
   - Update `**Current Step**` to the next step
   - Update `**Last Updated**` date
   - Add any deviations or discoveries to the Implementation Log
5. Plan next step and repeat

---

## Document Scoping Rules

**CRITICAL**: Each document has a specific scope. Do NOT duplicate content across documents.

| Content | Where it lives | NOT in |
|---------|---------------|--------|
| Status tracking, phase progress | SUMMARY.md only | Any other doc |
| Problem statement (brief) | SUMMARY.md | problem.md repeats it in detail |
| Success metrics | SUMMARY.md only | impact.md, architecture.md, testing.md |
| Risk assessment | SUMMARY.md only | impact.md, architecture.md, testing.md |
| Timeline | SUMMARY.md only | impact.md |
| Decision registry (what + one-line rationale) | SUMMARY.md only | — |
| Design rationale (alternatives, tradeoffs, why) | architecture.md only | SUMMARY.md |
| Acceptance criteria (project completeness) | SUMMARY.md only | — |
| Deferred scope | SUMMARY.md only | — |
| Current state + problems | problem.md only | SUMMARY.md has brief version |
| File-by-file impact + priorities | impact.md only | — |
| Dependency graph | impact.md only | — |
| Target architecture + interfaces | architecture.md only | — |
| Data flow + component design | architecture.md only | — |
| Test strategy + coverage goals | testing.md only | — |
| Rollout approach | testing.md only | — |

---

## Document Templates

### SUMMARY.md
The single source of truth for project overview, status, decisions, risks, and metrics.

```markdown
# [Project Name] - Plan Summary

## Overview
[2-3 sentence description of what this project does and why]

## Status
**Current Phase**: [Analysis/Design/Implementation]
**Current Step**: [Step N — description] or N/A
**Last Updated**: [Date]

| Phase | Status | Notes |
|-------|--------|-------|
| Analysis | [Not Started/In Progress/Complete] | |
| Design | [Not Started/In Progress/Complete] | |
| Implementation | [Not Started/In Progress/Complete] | |

### Implementation Steps
Convention: `[ ]` = not started, `[~]` = in progress, `[x]` = complete

- [ ] Step 1: [description]
- [ ] Step 2: [description]

## High-Level Work Areas
### P0 - Foundation
[Brief description of blocking work]

### P1 - Core Components
[Brief description]

### P2 - Integration
[Brief description]

### P3 - Polish
[Brief description]

## Key Decisions
1. **[Decision]**: [Choice made] — [rationale]
2. **[Decision]**: [Choice made] — [rationale]

## Success Metrics
### Quantitative
- [ ] [Metric]: [target]
- [ ] [Metric]: [target]

### Qualitative
- [ ] [Goal]
- [ ] [Goal]

## Risk Assessment
### High
- **[Risk]**: [Mitigation]

### Medium
- **[Risk]**: [Mitigation]

## Timeline
- **Phase 1**: [duration] — [description]
- **Phase 2**: [duration] — [description]

## Dependencies
- [Internal/external dependency]

## Deferred Scope
- [Item]: [reason for deferral]

## Acceptance Criteria
Define when this project is DONE — tied directly to the problem statement.
These are distinct from Success Metrics: acceptance criteria define completeness (problem is solved),
while success metrics measure quality (how well it was solved).

- [ ] [Criterion]: [how it maps to the problem statement]
- [ ] [Criterion]: [how it maps to the problem statement]

## Implementation Log
Record deviations, discoveries, and decisions made during implementation.
Update this after completing each step.

| Step | Date | Notes |
|------|------|-------|
| | | |

---
**Next Action**: [what happens next]
```

### 01-analysis/problem.md
Focused on: what exists, what's wrong, why it's wrong, proposed direction, and open questions.

```markdown
# Problem Analysis

## Problem Statement
[Clear description of what needs to be solved]

## Current State
### What Exists
[Inventory of existing components with file references]

### Current Capabilities
[What the system can do today]

## Problems and Gaps
### 1. [Problem Name]
[Description with file:line references]

### 2. [Problem Name]
[Description with file:line references]

## Root Cause Analysis
[Why these problems exist — technical, architectural, or business constraints]

## Proposed Approach
[2-5 sentences describing the high-level direction to address the problems above.
NOT a design — just a directional statement for the reviewer to approve or redirect.
Example: "Replace polling with an inotify/kqueue-backed watcher, compute diffs against
the index using gix, and render through a ratatui scroll surface that orders hunks by mtime."]

## Open Questions
[Questions that need answers before design can proceed]

---
**Status**: [Draft/Ready for Review]
**Phase**: Analysis
**Reviewed**: [Date or "Pending"]
```

### 01-analysis/impact.md
Focused on: file-by-file impact analysis and dependency ordering. No risk/metrics/timeline (those live in SUMMARY.md).

```markdown
# Impact Analysis

## Priority Classification
- **P0**: Core interfaces/abstractions that block other work
- **P1**: Shared utilities that unblock multiple components
- **P2**: Individual components depending on P0/P1
- **P3**: Cleanup after stability

## P0 - Critical
### [Component/Interface Name]
**Files**: [new or existing file paths]
**Description**: [what and why it's P0]
**Changes Needed**: [specific changes required — new file, rewrite, add methods, modify schema, etc.]
**Dependencies**: [what it depends on]
**Unblocks**: [what depends on it]

## P1 - High
[Same structure as P0]

## P2 - Medium
[Same structure as P0]

## P3 - Low
[Same structure as P0]

## Dependency Graph
```
component-a (P0) — blocks everything
    ↓
component-b (P1) ← depends on a, blocks c and d
    ↓
component-c (P2) ← depends on b
    ↓
component-d (P3) ← cleanup, depends on c being stable
```

## File-by-File Summary
| File | Action | Priority | Depends On |
|------|--------|----------|------------|
| `src/watcher.rs` | [New/Rewrite/Update] | P0 | — |
| `src/diff.rs` | [New/Rewrite/Update] | P1 | watcher.rs |

---
**Status**: [Draft/Ready for Review]
**Phase**: Analysis
**Reviewed**: [Date or "Pending"]
```

### 02-design/architecture.md
Focused on: design rationale, data model, target architecture, components, interfaces, data flow, migration path. No risk/metrics/deployment (those live in SUMMARY.md and testing.md).

```markdown
# Architecture Design

## Architectural Principles
[3-5 key principles driving design decisions]

## Design Rationale
For each significant design choice, explain why this approach was chosen.
SUMMARY.md records WHAT was decided; this section explains WHY from a design perspective.

### [Choice Name]
**Decision**: [what was chosen]
**Alternatives considered**: [other options evaluated]
**Why this approach**: [tradeoffs that made this the right choice]

### [Choice Name]
[Same structure]

## System Architecture
```
[ASCII diagram of high-level components and their relationships]
```

## Data Model
### In-Memory State
```rust
// Key structs, enums, and their relationships
```

### Key Data Decisions
[Why this structure — ownership, lifetimes, allocation strategy, sync vs async boundaries]

## Component Design
### [Component Name]
**Purpose**: [what it does]
**Responsibilities**: [specific responsibilities]
**Dependencies**: [other components this depends on, or "None"]

### [Component Name]
[Same structure]

## Interface Specifications
```rust
// Key traits and types
pub trait Watcher {
    fn poll(&mut self) -> Result<Vec<Event>, Error>;
}

pub struct Diff {
    pub path: PathBuf,
    pub mtime: SystemTime,
    pub hunks: Vec<Hunk>,
}
```

## Data Flow
### [Flow Name]
```
[Step-by-step data flow: trigger → processing → storage → response]
```

## Migration Path
1. [Phase 1 changes]
2. [Phase 2 changes]

## Future Extensibility
[How the architecture supports future growth without current over-engineering]

---
**Status**: [Draft/Ready for Review]
**Phase**: Design
**Reviewed**: [Date or "Pending"]
```

### 02-design/testing.md
Focused on: what to test at each level, coverage goals, rollout strategy. No full test code (that belongs in implementation steps).

**Guiding principle**: Only propose high-value tests. Every proposed test must justify its existence with a clear rationale explaining what risk it mitigates, what behavior it verifies, or what regression it prevents. Do not propose tests for trivial getters/setters, simple wiring, or framework behavior that is already tested upstream.

```markdown
# Testing & Rollout Strategy

## Testing Approach
### Unit Tests
**What to test**: [specific areas — diff computation, mtime ordering, error handling]
**What to skip**: [areas better covered by integration tests or too trivial to warrant testing]
**Convention**: Inline `#[cfg(test)] mod tests` blocks alongside the code under test.

### Integration Tests
**What to test**: [end-to-end watcher → diff → render flows against real git repos]
**Convention**: Files under `tests/` directory. Gate slow/IO-heavy suites behind `--ignored` or a Cargo feature when appropriate.

### Benchmarks
**What to bench**: [hot paths — diff recompute on event, fanout cost per event, render frame cost]
**Convention**: `benches/` with Criterion. Track regressions explicitly — this project is performance-sensitive.

### Coverage Goals
- [Area]: [target]% — [rationale for this target]
- [Area]: [target]% — [rationale for this target]

## Key Test Scenarios
Only include scenarios that protect against meaningful failure modes. Each scenario must include a rationale.

### [Scenario Name]
- **Rationale**: [why this test matters — what risk does it mitigate, what regression does it prevent, what complex behavior does it verify]
- **Setup**: [what state is needed]
- **Action**: [what is being tested]
- **Expected**: [what should happen]

### [Scenario Name]
[Same structure — must include Rationale]

## Rollout Strategy
### Release Approach
[How new code will be released — cargo publish, tagged releases, prebuilt binaries]

### Rollback Plan
[How to revert if a release goes wrong — yank, point release, etc.]

---
**Status**: [Draft/Ready for Review]
**Phase**: Design
**Reviewed**: [Date or "Pending"]
```

### 03-implementation/step-NN-description.md
Each step is self-contained and executable by anyone.

```markdown
# Step N: [Title]
**Status**: [Not Started/In Progress/Complete]

## Objective
[What this step accomplishes]

## Context
[What exists after the prerequisite step was completed. Key decisions from SUMMARY.md that affect this step. Brief enough to orient an AI starting a fresh session.]

## Scope
[Bullet list of what's included and what's explicitly excluded]

## Files to Create/Modify
### [File Path]
**Action**: [New/Modify/Replace]
**Purpose**: [Why this change is needed]

```rust
// Key code showing traits, types, or critical logic
```

### [File Path]
[Same structure]

## Implementation Steps
1. [Concrete step with commands or specific instructions]
2. [Next step]
3. [Next step]

## Validation Criteria
- [ ] [Specific, testable criterion]
- [ ] [Specific, testable criterion]
- [ ] [Specific, testable criterion]

## Prerequisite
[Previous step that must be complete, or "None"]

## Next Step
[Brief pointer to what comes next]

---
**Status**: [Not Started/In Progress/Complete]
**Phase**: Implementation
**Reviewed**: [Date or "Pending"]
```

---

## Analysis Completion Criteria

### Signs You're Not Done
- Using words like "probably", "might", "I think" about scope or impact
- Files you haven't opened but "should be fine"
- Patterns you've seen once but haven't searched for systematically
- Dependencies you're assuming rather than verifying

### Signs You're Over-Analyzing
- Finding the same patterns repeatedly with no new insights
- Analysis documents growing but no new categories of issues emerging
- Diminishing returns on additional code searches

## Iteration and Loop-Back Guidance

**Loop back to Design when:**
- Implementation reveals the interface design doesn't work in practice
- Performance characteristics differ significantly from assumptions
- A cleaner architectural approach becomes obvious during implementation

**Loop back to Analysis when:**
- Implementation uncovers a major area of impact that was missed
- New dependencies are discovered that change the scope significantly

**How to loop back:**
1. Stop implementation
2. Document the discovery in the relevant phase document
3. Update SUMMARY.md with revised status
4. Request review before resuming

## Scope Change Protocol

When scope changes, handle explicitly:
1. Document what changed and why in SUMMARY.md
2. Assess whether to proceed as one effort or split
3. Update affected documents (don't just patch)
4. Request review for the changed scope

---

**Last Updated**: 2026-05-02
**Status**: Active approach for gitstream non-trivial planning
