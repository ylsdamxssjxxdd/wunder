---
title: Companion System
summary: Use sprite-animated companions on desktop and web so agents accompany your work in a livelier way.
---

# Companion System

The companion system lets you give an agent a cute sprite-animated character. The companion floats on screen, plays different animations based on the agent's state, and can display message bubbles.

## When It Helps

Companions fit these scenarios:

- **Long-running tasks**: while the agent works on something complex, the companion shows a "running" animation so you know it is busy
- **Desktop company**: on the desktop app the companion stays visible, like a little assistant beside your work
- **Telling agents apart**: different agents can have different companions, easy to distinguish at a glance

## Enabling Companions

### Desktop

1. Open the settings panel
2. Find the "Companion" setting
3. Turn on "Enable companion"
4. Pick a companion from the list

### Web

1. Click your avatar in the top-right corner
2. Go to "Preferences"
3. Find the "Companion" section
4. Enable and pick a companion

## Where Companions Come From

### Global companions

Uploaded by system administrators, visible to all users. These are usually reviewed and of reliable quality.

### Private companions

Companions you import yourself, available only in your browser.

**How to import:**

1. Prepare a companion package (ZIP file)
2. Click "Import companion" in the companion settings
3. Choose the ZIP file
4. It is selected automatically after a successful import

## Companion Package Format

To create your own companion, prepare:

### File structure

```
my-companion.zip
├── pet.json        # manifest
└── spritesheet.webp # sprite sheet image
```

### Manifest (pet.json)

```json
{
  "id": "my-companion",
  "displayName": "My Companion",
  "description": "A custom sprite companion",
  "spritesheetPath": "spritesheet.webp"
}
```

### Sprite sheet requirements

- **Formats**: WebP, PNG, GIF, or JPEG
- **Size**: max 18MB per file
- **Package**: max 24MB for the whole ZIP

### Sprite sheet layout

The sprite sheet is a vertically stacked frame sequence; the system picks rows by state:

- Row 1: idle
- Row 2: running
- Row 3: waving
- Row 4: jumping
- Row 5: failed
- Row 6: waiting
- Row 7: review

## Interacting with Companions

### Click

Clicking a companion plays the waving animation and shows its name.

### Drag

Dragging moves the companion around the screen. The position is saved automatically.

### Context menu

Right-clicking opens a menu:

- **Open conversation**: jump to that agent's chat
- **Show/hide**: toggle companion visibility
- **Scale**: adjust size (0.5x - 1.6x)

## Message Bubbles

When an agent replies, a bubble appears above its companion:

- The bubble shows a summary of the latest reply
- It disappears automatically
- Bubbles can be turned off in settings

## Setting a Companion for an Agent

### Option 1: Agent settings panel

1. Open the agent details
2. Find the "Icon / companion" setting
3. Choose "Use companion"
4. Pick a companion from the dropdown
5. Adjust display options (show/hide, scale)

### Option 2: Edit agent configuration

Set the `icon` field in the agent's configuration:

```json
{
  "icon": {
    "kind": "companion",
    "scope": "global",
    "id": "cat-assistant",
    "show": true,
    "scale": 1.0,
    "messageHints": true
  }
}
```

## FAQ

### The companion does not show

Check:

1. Is the companion feature enabled?
2. Is a companion selected?
3. Is the companion hidden for that agent?
4. Does the browser support WebGL (some older browsers may not)?

### Import fails

Common causes:

- ZIP file exceeds 24MB
- Sprite sheet image exceeds 18MB
- `pet.json` is malformed
- Required fields are missing (id, displayName, spritesheetPath)

### Companion position resets

The position is stored in browser local storage. Clearing browser data resets it to the default position.

## Admin Operations

If you are a system administrator, upload global companions through the admin console:

1. Go to "System settings" → "Companion management"
2. Click "Upload companion"
3. Choose the companion ZIP package
4. After upload it is visible to all users

You can also edit a companion's display name and description, or delete ones you no longer need.

## Next Steps

- [Goal Mode](/docs/en/concepts/goal-mode/) — keep agents working until the goal is done
- [Agent Loop](/docs/en/concepts/core-agent-loop/) — how agents execute tasks
