---
name: skill-creator
title: Skill Authoring & Validation Utility
description: Specialized assistant for authoring, linting, validating, and testing custom Omni-Harness skills.
command: /skill-create
version: 1.0.0
author: System
tags:
  - meta
  - skill
  - workflow
keywords:
  - make skill
  - create skill
  - new skill
  - skill validator
auto_start: false
enabled: true
trusted: true
---

# Skill Creator

## When to use
Activate this skill whenever the user asks to create, update, or validate an Omni-Harness skill bundle.

## Inputs
- Desired skill name, command shortcut, and domain description.
- Workflows, tool requirements, and expected operational parameters.

## Rules
- Every skill must have valid YAML frontmatter between triple dashes.
- All 7 mandatory H2 headings must be present.
- Total skill file size must not exceed safety thresholds.

## Workflow
1. Gather the target requirements from the user request.
2. Draft the YAML frontmatter with tags and matching keywords.
3. Complete each of the seven required sections thoroughly.
4. Verify section headers against REQUIRED_SECTIONS.
5. Save the generated SKILL.md into the designated skills subfolder.

## Output
A validated SKILL.md file and optional helper scripts inside scripts/ directory.

## Assets and scripts
- scripts/validate_skill.py
- assets/template_skill.md

## Validation
Ensure that `validate_skill_sections` passes with 0 missing sections and file size is under 2 MB.
