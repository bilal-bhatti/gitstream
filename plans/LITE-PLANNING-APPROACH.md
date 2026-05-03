# gitstream Lite Planning Approach (One-Pager)

For smaller tasks that don't warrant the full structured approach but still benefit from critical thinking and planning.

## Purpose
Force the same analytical rigor — understand the problem before solving it, consider tradeoffs, identify risks — but capture everything in a single document. This file serves as the AI's external memory for the task and as a reviewable artifact for the human.

## Process
1. Analyze the problem and existing code thoroughly before writing the plan
2. Create a single file: `plans/<task-name>/<task-name>.md` (always use a subdirectory, never place plan files directly in `plans/`)
3. **FORMAL REVIEW REQUIRED** — Do not implement until the plan is approved
4. During implementation, update the Status and Implementation Notes sections
5. Mark complete when done

## Template

```markdown
# [Task Name]

**Status**: [Draft/Approved/In Progress/Complete]
**Last Updated**: [Date]

## Problem
[What needs to change and why. Reference specific files and code. 2-5 sentences.]

## Current State
[What exists today that's relevant. File paths, current behavior, key constraints. Be specific.]

## Approach
[How you'll solve it. Key design choices and why. If there were alternatives, briefly note why this approach was chosen over them.]

## Design Details
Include when the approach involves new traits, data structures, or non-obvious data flow. Show the concrete design — trait signatures, struct definitions, or data flow diagrams. Keep it focused on what the reviewer needs to evaluate the design's soundness.

## Impact
[Files to create/modify with what changes. Use a table for 4+ files.]

| File | Action | Change |
|------|--------|--------|
| `path/to/file` | [New/Modify] | [what changes] |

## Risks
[What could go wrong and how you'll handle it. Skip if genuinely none.]

## Validation
- [ ] [How you'll verify the change works]
- [ ] [How you'll verify nothing broke]

## Implementation Notes
[Updated during implementation. Record deviations, discoveries, decisions made.]
```

## Guidelines
- **Be specific, not generic** — reference actual files, functions, and behaviors
- **Think critically** — don't just describe the task, analyze it. Why this approach? What are the tradeoffs?
- **Keep it honest** — if you're uncertain about something, say so rather than glossing over it
- **Update as you go** — the Implementation Notes section is your memory across context boundaries

---

**Last Updated**: 2026-05-02
**Status**: Active approach for gitstream lite planning
