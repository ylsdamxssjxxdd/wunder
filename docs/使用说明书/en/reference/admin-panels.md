---
title: Admin Panels
summary: The admin-side frontend is not a single debug page, but a set of well-organized governance panels with clear responsibilities.
read_when:
  - You are using the admin side for the first time
  - You want to know where to find a specific configuration, monitoring view, or documentation entry
source_docs:
  - docs/系统介绍.md
  - docs/设计方案.md
  - web/index.html
---

# Admin Panels

The key value of the admin-side frontend is not "cram everything onto one page", but grouping governance entry points by function.

## System Group

This group is oriented toward global governance:

- Internal Status
- System Settings
- Organization Management
- User Management
- Link Management
- Channel Monitoring

If you need to check runtime state, organizational structure, or global configuration, start here.

### User Management Supplement

The settings dialog in User Management now also handles Token account governance:

- You can directly modify a regular user's Token balance
- You can issue or deduct Tokens with proper accounting semantics
- Deductions validate the available balance to prevent over-drafting
- Admin accounts are not subject to these balance restrictions

## Agent Group

This group is oriented toward conversation and capability governance:

- Model Configuration
- Thread Management
- Preset Agents
- Tool Management
- System Prompt Templates

If you need to see "how models are configured and exposed", start here.

## Debug Group

This group is oriented toward engineering validation:

- Throughput Testing
- Performance Testing
- Swarm Testing
- Capability Evaluation
- Debug Panel
- LSP

If you are doing integration testing, load testing, or capability verification, this group is the most important.

## Documentation Group

The admin side now groups documentation entries separately, mainly including:

- User Guide
- API Reference

They serve different purposes.

### User Guide

This is an embedded entry point to the public documentation site.

Good for looking up:

- Getting Started
- Core Concepts
- Integration
- Operations
- Reference

### API Reference

Good for:

- Quickly looking up `/wunder` and related endpoints
- Viewing fields, examples, and integration methods

## What Different Roles Use Most

### Administrators

Most frequently use:

- System Settings
- User Management
- Channel Monitoring
- User Guide

### Developers

Most frequently use:

- Debug Panel
- API Reference
- Tool Management
- Performance Testing

### Delivery or Training Staff

Most frequently use:

- User Guide

## A Practical Tip

If you are not sure where to start investigating a problem, follow this order:

1. Check the User Guide for existing documentation
2. Check the API Reference for field definitions
3. Go to the relevant governance panel to verify

## Further Reading

- [Admin Interface](/docs/en/surfaces/web-admin/)
- [Configuration Reference](/docs/en/reference/config/)
- [Stream Events Reference](/docs/en/reference/stream-events/)
