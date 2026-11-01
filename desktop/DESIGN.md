---
version: alpha
name: "Arca Precision Design System"
description: "Production design tokens, component architecture, and styling rules for Arca Desktop and CLI. Enforces high-density precision engineering, Swiss typography, and deep Obsidian/Porcelain themes across human contributors and AI coding agents."

colors:
  # Brand Accents
  brand-blue: "#2563EB"
  brand-blue-hover: "#1D4ED8"
  brand-blue-press: "#1E40AF"
  brand-blue-dark: "#3B82F6"
  brand-blue-dark-hover: "#60A5FA"
  brand-gold: "#F59E0B"
  brand-gold-subtle: "#D97706"
  brand-green: "#10B981"
  brand-green-dark: "#22C55E"
  brand-red: "#DC2626"
  brand-red-dark: "#EF4444"

  # Dark Theme: Obsidian Pro Architecture
  dark-surface-app: "#08090C"
  dark-surface-canvas: "#0A0B0E"
  dark-surface-frame: "#090A0D"
  dark-surface-navigation: "#0D0E12"
  dark-surface-workspace: "#0A0B0E"
  dark-surface-section: "#12131A"
  dark-surface-asset: "#121318"
  dark-surface-item: "#15161D"
  dark-surface-control: "#161820"
  dark-surface-selected: "#142036"
  dark-surface-hover: "#171922"
  dark-surface-popover: "#161821"
  dark-border-hairline: "#1F222B"
  dark-border-subtle: "#171921"
  dark-text-primary: "#EDEDF0"
  dark-text-secondary: "#9BA3AF"
  dark-text-tertiary: "#636B78"

  # Light Theme: Swiss Porcelain
  light-surface-app: "#F5F6F8"
  light-surface-canvas: "#FFFFFF"
  light-surface-frame: "#F6F7F9"
  light-surface-navigation: "#F8F9FA"
  light-surface-workspace: "#FFFFFF"
  light-surface-section: "#EDEFF3"
  light-surface-asset: "#FFFFFF"
  light-surface-item: "#F1F3F6"
  light-surface-control: "#FFFFFF"
  light-surface-selected: "#EFF6FF"
  light-surface-hover: "#F8F9FB"
  light-surface-popover: "#FFFFFF"
  light-border-hairline: "#E2E5EB"
  light-border-subtle: "#ECEFF4"
  light-text-primary: "#090B10"
  light-text-secondary: "#475467"
  light-text-tertiary: "#667085"

typography:
  fontFamily:
    sans: '-apple-system, BlinkMacSystemFont, "SF Pro Text", "PingFang SC", system-ui, sans-serif'
    mono: '"SF Mono", ui-monospace, "Menlo", monospace'

  scales:
    page-title:
      fontFamily: "{typography.fontFamily.sans}"
      fontSize: "20px"
      fontWeight: "700"
      lineHeight: "1.2"
      letterSpacing: "-0.02em"
    section-title:
      fontFamily: "{typography.fontFamily.sans}"
      fontSize: "13px"
      fontWeight: "660"
      lineHeight: "1.3"
      letterSpacing: "-0.01em"
    card-title:
      fontFamily: "{typography.fontFamily.sans}"
      fontSize: "13.5px"
      fontWeight: "660"
      lineHeight: "1.3"
      letterSpacing: "-0.01em"
    body:
      fontFamily: "{typography.fontFamily.sans}"
      fontSize: "11.5px"
      fontWeight: "400"
      lineHeight: "1.5"
    subline:
      fontFamily: "{typography.fontFamily.mono}"
      fontSize: "11px"
      fontWeight: "500"
      lineHeight: "1.35"
    badge:
      fontFamily: "{typography.fontFamily.mono}"
      fontSize: "10px"
      fontWeight: "650"
      lineHeight: "1"
      letterSpacing: "0.04em"

spacing:
  2xs: "2px"
  xs: "4px"
  sm: "6px"
  md: "8px"
  base: "10px"
  card-gap: "12px"
  card-pad-y: "13px"
  card-pad-x: "15px"
  lg: "16px"
  xl: "20px"
  2xl: "24px"

rounded:
  pill: "5px"
  badge: "5px"
  action: "7px"
  control: "8px"
  avatar: "10px"
  card: "12px"
  region: "14px"
  dialog: "16px"
  full: "9999px"

elevation:
  shadow-card-dark: "0 1px 3px rgba(0, 0, 0, 0.55), inset 0 1px 0 0 rgba(255, 255, 255, 0.045)"
  shadow-hover-dark: "0 8px 24px rgba(0, 0, 0, 0.72), inset 0 1px 0 0 rgba(255, 255, 255, 0.07)"
  shadow-sheet-dark: "0 28px 72px rgba(0, 0, 0, 0.85), 0 6px 20px rgba(0, 0, 0, 0.5)"
  shadow-card-light: "0 1px 3px rgba(15, 23, 42, 0.04), 0 1px 2px rgba(15, 23, 42, 0.02)"
  shadow-hover-light: "0 6px 18px rgba(15, 23, 42, 0.07), 0 2px 4px rgba(15, 23, 42, 0.03)"
  shadow-sheet-light: "0 20px 48px rgba(15, 23, 42, 0.14), 0 4px 12px rgba(15, 23, 42, 0.06)"

components:
  resource-card:
    minHeight: "114px"
    padding: "13px 15px 10px"
    borderRadius: "{rounded.card}"
    divider: "1px solid {colors.dark-border-subtle}"
    hoverTransform: "translateY(-1px)"
  avatar-tile:
    size: "36px"
    borderRadius: "{rounded.avatar}"
    iconScale: "62%"
  agent-mcp-card:
    minHeight: "68px"
    borderRadius: "{rounded.card}"
    padding: "11px 14px"
  agent-model-card:
    minHeight: "124px"
    borderRadius: "{rounded.card}"
    padding: "13px 15px"
---

# Arca Precision Design System (`DESIGN.md`)

> **Authoritative Specification**: This file is the single source of truth for all visual design tokens, component architecture, and styling constraints for the **Arca** desktop application and related tools.
> AI coding agents and human developers must adhere to these tokens and constraints to prevent visual drift and maintain high-density craft standards.

---

## 1. Overview

Arca is a high-performance cross-agent management hub for developers. The aesthetic language combines **Swiss International Typographic Style** with **dark architectural precision engineering** (influenced by Linear, Raycast, and Zed).

### Core Pillars
1. **Physical Material Integrity**: No fake glassy blur blobs or exaggerated saturated neon glows. Surfaces follow a real physical hierarchy with specular top highlights (`inset 0 1px 0 0`) and dark hairline borders.
2. **Deterministic Geometry**: All card layouts adhere to strict mathematical heights, unified `12px` border-radii, and a 4px layout rhythm.
3. **Zero Visual Slop**: Symmetrical icon geometry, strict tabular numerals for metrics, and restrained accent color allocation.

---

## 2. Colors & Themes

Arca provides two first-class color spaces: **Obsidian Pro** (Dark) and **Porcelain** (Light). Contrast ratios must always meet or exceed **WCAG AA** standards (4.5:1 for body text, 3:1 for large text/icons).

### Theme Surface Hierarchy

```
Layer             Dark (Obsidian Pro)      Light (Porcelain)        Purpose
─────────────────────────────────────────────────────────────────────────────────────────────
App Base          #08090C                  #F5F6F8                  Window root background
Frame / Nav       #090A0D / #0D0E12        #F6F7F9 / #F8F9FA        Topbar, titlebar, sidebar
Workspace Stage   #0A0B0E                  #FFFFFF                  Main canvas workspace
Asset Card        #121318                  #FFFFFF                  Interactive resource cards
Elevated Item     #15161D                  #F1F3F6                  Input fields, pill containers
Active / Select   #142036                  #EFF6FF                  Selected card / row state
Popover / Modal   #161821                  #FFFFFF                  Floating dialogs & dropdowns
```

### Hairline & Border Contracts
- **`--border-hairline`** (`#1F222B` Dark / `#E2E5EB` Light): Exterior card boundaries and dialog borders.
- **`--border-subtle`** (`#171921` Dark / `#ECEFF4` Light): Internal dividers (e.g. card meta splitters).

### Semantic & Domain Accents
- **Primary / Actions**: `#3B82F6` (Dark) / `#2563EB` (Light). Used for primary buttons, active tabs, and focus rings.
- **Skills Domain**: `#F59E0B` (Amber Gold). Reserved for Skill avatars, badges, and provenance indicators.
- **MCPs Domain**: Semantic category glyphs tinted via `currentColor` with subtle category tones.
- **Models Domain**: Official provider color identities (OpenAI `#10A37F`, Anthropic `#D97706`, Google `#4285F4`, DeepSeek `#0066FF`).
- **Success / Effective**: `#22C55E` / `#16A34A` (Dot & badge text).
- **Warning / Attention**: `#F59E0B` / `#D97706`.
- **Danger / Delete**: `#EF4444` / `#DC2626`.

---

## 3. Typography

Arca relies exclusively on system-native font stacks for zero font-loading latency and crisp native rendering.

### Font Stacks
```css
--font-sans: -apple-system, BlinkMacSystemFont, "SF Pro Text", "PingFang SC", system-ui, sans-serif;
--font-mono: "SF Mono", ui-monospace, "Menlo", monospace;
```

### Typographic Hierarchy Matrix

| Element | Font Family | Size | Weight | Line Height | Tracking | Case / Alignment |
|---|---|---|---|---|---|---|
| **View Heading** | `sans` | `20px` | `700` | `1.2` | `-0.02em` | Left-aligned |
| **Section Title** | `sans` | `13px` | `660` | `1.3` | `-0.01em` | Primary text |
| **Card Primary Title** | `sans` | `13.5px` | `660` | `1.3` | `-0.01em` | Clamped 1-line |
| **Model / Endpoint ID** | `mono` | `11px` | `500` | `1.35` | `0` | Truncated ellipsis |
| **Card Description** | `sans` | `11.5px` | `400` | `1.5` | `0` | Clamped 1 or 2 lines |
| **Pill / Meta Text** | `sans` | `10.5px` | `570` | `1.0` | `0` | Center-aligned |
| **Badge / Transport** | `mono` | `10px` | `650` | `1.0` | `0.04em` | All-caps tabular |
| **Counter / Tabular Num** | `sans` | `11px` | `500` | `1.0` | `tabular` | Tabular numbers |

---

## 4. Layout & Spacing

### 4px Baseline Rhythm
All margins, paddings, and gap dimensions must be multiples of 2px/4px:
`2px`, `4px`, `6px`, `8px`, `10px`, `12px`, `14px`, `16px`, `20px`, `24px`, `32px`.

### Responsive Breakpoints & Viewport Contracts
- **Primary Desktop Standard**: `1200px × 820px` (2-column library card grids, 3-column Agent consumption grid).
- **Compact Window Standard**: `900px × 600px` (Dense responsive grid with horizontal scrolling prevention).
- **Container Query**: `@container resource-stage (max-width: 780px)` collapses library grids to 1 column.

---

## 5. Elevation & Depth

Arca rejects blurry ambient drop shadows in favor of **precision surface physics**:

1. **Specular Top Highlight**:
   ```css
   /* Dark Mode Card Highlight */
   box-shadow: 0 1px 3px rgba(0, 0, 0, 0.55), inset 0 1px 0 0 rgba(255, 255, 255, 0.045);
   ```
   Provides a crisp 1px specular bevel on top of every card mimicking precision-milled hardware.

2. **Hover Elevation**:
   ```css
   transform: translateY(-1px);
   box-shadow: 0 8px 24px rgba(0, 0, 0, 0.72), inset 0 1px 0 0 rgba(255, 255, 255, 0.07);
   border-color: color-mix(in srgb, var(--color-blue) 42%, var(--border-hairline));
   ```

3. **Layering Z-Index Registry**:
   - Base workspace: `z-index: 1`
   - Hovered/focused card: `z-index: 3`
   - Popovers / Menus: `z-index: 8`
   - Modal Dialogs (`DialogShell`): `z-index: 100`
   - Global Toast Notifications (`ToastNotice`): `z-index: 200`

---

## 6. Shapes & Radii

Every radius has an intentional semantic purpose. Arbitrary radii (such as 6px, 9px, or 11px) are prohibited.

```
Radius      Value       Applied Components
────────────────────────────────────────────────────────────────────────────────
pill        5px         Protocol pill, transport badge, status tags
action      7px         Icon-only action buttons (terminal curl, trash, edit)
control     8px         Text inputs, selects, form triggers, badge bases
avatar      10px        36px x 36px icon avatars (Provider, MCP, Skill)
card        12px        All resource cards (Models, MCPs, Skills, Agent consumption)
region      14px        Sidebar containers, nested grouped sections
dialog      16px        System modals, dialog windows, outer app canvas
full        9999px      Status indicator dots, capsule buttons, switches
```

---

## 7. Component Architecture & Anatomy

### 7.1 Unified 2-Tier Resource Cards (`Models`, `MCPs`, `Skills`)
All 3 library cards follow an identical structural blueprint:

```
┌────────────────────────────────────────────────────────────────────────┐
│ [ 36px Avatar ]  Title String (13.5px / 660)       [ Status / Actions ]│
│                  Identifier / Subline (11px Mono)                      │
│                  Clamped 1-line description (Skills only)              │
├────────────────────────────────────────────────────────────────────────┤  <-- 1px hairline divider
│  [ Pill / Transport ]  [ Source Text ]             [ Consumer Stack ] │
└────────────────────────────────────────────────────────────────────────┘
```

- **Container Dimensions**: `min-height: 114px; padding: 13px 15px 10px; border-radius: 12px;`
- **Top Section**:
  - `Avatar Tile`: 36px × 36px with 10px radius and centered 20px glyph.
  - `Identity Copy`: Title with truncation + subline code.
  - `Trailing Action`: Action buttons (`28px-30px`) or Status Badge (`effective`/`shadowed`).
- **Hairline Divider**: `border-top: 1px solid var(--border-subtle)` spanning 100% full card width.
- **Bottom Section**: Meta pills on the left, Agent consumers (Stack or Fan) on the right.

### 7.2 Agent Workspace Consumption Cards (`AgentView`)
- **MCP Consumption Card**:
  - Dimensions: `min-height: 68px; padding: 11px 14px; border-radius: 12px;`
  - Horizontal single-tier layout: 32px Icon + Copy + Green iOS-style Switch + Menu Button.
- **Model Consumption Card**:
  - Dimensions: `min-height: 124px; padding: 13px 15px; border-radius: 12px;`
  - 36px Icon + Active Radio Indicator + Model Selector.

### 7.3 Avatars & Semantic Glyphs
- ViewBox Standard: All custom icons must use strict square coordinate boxes (e.g. `viewBox="0 0 16 16"` or `viewBox="0 0 24 24"`).
- Stroke Width: `1.5px` (for 16px/18px icons) or `1.75px` (for 24px icons), with `strokeLinecap="round"` and `strokeLinejoin="round"`.

### 7.4 Dialogs & Modals
- Mounted strictly to `document.body` via `createPortal`.
- Mask: `background: var(--surface-modal-scrim)` with subtle backdrop blur.
- Structure: Fixed 56px Header + Scrollable Body + Fixed 52px Footer with primary action on the right.

### 7.5 Toasts & Feedback
- Positioned in top-right viewport: `top: 18px; right: 20px; z-index: 200;`.
- Entry animation: Horizontal slide from right (`translateX(100%) -> 0`) in 180ms cubic-bezier.

---

## 8. Do's and Don'ts

### Colors & Styling
- ❌ **DON'T** use pure `#000000` for dark mode backgrounds. Always use Obsidian `#08090C` / `#0A0B0E`.
- ❌ **DON'T** use saturated rainbow gradients or decorative blurred blobs under cards.
- ❌ **DON'T** introduce unapproved brand colors. All accents must trace to Blue, Gold, Green, or Red.
- ✅ **DO** use `color-mix(in srgb, ...)` for state variations (hover, active, focus).
- ✅ **DO** ensure all text colors map to `--text-primary`, `--text-secondary`, or `--text-tertiary`.

### Layout & Geometry
- ❌ **DON'T** allow card heights to drift between resource types. Keep library cards locked to `114px`.
- ❌ **DON'T** use random border radii (e.g., 6px, 9px, 15px). Use the standard scale: 5px, 7px, 8px, 10px, 12px, 16px.
- ❌ **DON'T** place metadata directly beneath text without a full-width hairline divider.
- ✅ **DO** clamp all long names and descriptions to prevent overflowing card boundaries.
- ✅ **DO** verify all layouts at both `1200x820` and `900x600` viewports without horizontal overflow.

### Icons & Assets
- ❌ **DON'T** render icons with mismatched sizes (e.g. 18px next to 14px in topbar).
- ❌ **DON'T** place plain text headers in cards without their corresponding 36px avatar tile.
- ✅ **DO** ensure icons scale proportionally inside their parent tiles (`62%` dimension bounding).
- ✅ **DO** keep tab navigation icons locked to `14px × 14px`.
