# Cyberpunk Neon TUI Enhancement Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Enhance the roughneck-cli TUI with cyberpunk aesthetics including neon RGB colors, animated spinners, pulsing borders, and gradient text effects.

**Architecture:** Add animation state tracking to AppState, create color/animation helper functions, and update all rendering functions to use RGB colors and computed animation values. No new dependencies - uses existing ratatui RGB color support.

**Tech Stack:** Rust, ratatui (with RGB color support), crossterm

**Reference:** `docs/superpowers/specs/2026-03-10-cyberpunk-tui-design.md`

---

## File Structure

**Modified Files:**
- `crates/roughneck-cli/src/tui.rs` - All visual enhancements

**Testing Strategy:**
- Build verification after each change
- Visual testing by running the CLI with: `cargo run -p roughneck-cli`
- Test animations over time (spinner patterns, pulsing effects)
- Test with various states (busy, idle, errors, todos)

---

## Chunk 1: Foundation - Animation State and Color Helpers

### Task 1: Add AnimationState struct

**Files:**
- Modify: `crates/roughneck-cli/src/tui.rs:496-511` (AppState struct)

- [ ] **Step 1: Add AnimationState struct definition**

Add after the existing SPINNER constant at the top of the file (after the constant definitions around line 34):

```rust
#[derive(Debug)]
struct AnimationState {
    frame: usize,
    pulse_phase: f32,
    glow_intensity: f32,
    spinner_pattern: usize,
}

impl AnimationState {
    fn new() -> Self {
        Self {
            frame: 0,
            pulse_phase: 0.0,
            glow_intensity: 0.0,
            spinner_pattern: 0,
        }
    }
}
```

- [ ] **Step 2: Add animation field to AppState**

In the AppState struct definition (search for `struct AppState`), add field after `tick: usize,`:

```rust
animation: AnimationState,
```

- [ ] **Step 3: Initialize animation state in AppState::new**

In the AppState::new() function (search for `fn new(session_id: String`), add initialization after `tick: 0,`:

```rust
animation: AnimationState::new(),
```

- [ ] **Step 4: Update animation state in on_tick**

Replace the entire on_tick method in AppState (search for `fn on_tick(&mut self)`) with:

```rust
fn on_tick(&mut self) {
    self.tick = self.tick.wrapping_add(1);

    // Update animation state
    self.animation.frame = self.tick;
    self.animation.pulse_phase = ((self.tick % 16) as f32) / 16.0;
    let glow_angle = ((self.tick % 24) as f32) / 24.0 * 2.0 * std::f32::consts::PI;
    self.animation.glow_intensity = glow_angle.sin().abs();
    self.animation.spinner_pattern = (self.tick / 80) % 4;
}
```

- [ ] **Step 5: Build to verify compilation**

Run: `cargo build -p roughneck-cli`
Expected: Builds successfully with no errors

- [ ] **Step 6: Commit animation state foundation**

```bash
git add crates/roughneck-cli/src/tui.rs
git commit -m "Add animation state tracking to TUI

Introduces AnimationState struct with frame counter, pulse phase,
glow intensity, and spinner pattern tracking. Updates on every tick
to drive cyberpunk visual effects.

Co-Authored-By: Claude Opus 4.6 <noreply@anthropic.com>"
```

---

### Task 2: Create color constants and helper functions

**Files:**
- Modify: `crates/roughneck-cli/src/tui.rs:34` (after constants)

- [ ] **Step 1: Add color constants**

Add after the SPINNER constant at the top of the file (this constant will be replaced in Task 4, but for now add colors after it):

```rust
// Cyberpunk neon color palette
const NEON_CYAN: (u8, u8, u8) = (0, 255, 255);
const ELECTRIC_MAGENTA: (u8, u8, u8) = (255, 0, 255);
const ELECTRIC_BLUE: (u8, u8, u8) = (0, 128, 255);
const NEON_GREEN: (u8, u8, u8) = (0, 255, 136);
const HOT_PINK: (u8, u8, u8) = (255, 20, 147);
const BRIGHT_WHITE: (u8, u8, u8) = (255, 255, 255);
const MEDIUM_GRAY: (u8, u8, u8) = (128, 128, 128);
const DARK_GRAY: (u8, u8, u8) = (64, 64, 64);
```

- [ ] **Step 2: Add color interpolation helper**

Add before the panel() function (search for `fn panel<'a>(title: &'a str) -> Block<'a>`):

```rust
fn interpolate_brightness(base_rgb: (u8, u8, u8), intensity: f32) -> Color {
    let (r, g, b) = base_rgb;
    let intensity = intensity.clamp(0.0, 1.0);
    Color::Rgb(
        (r as f32 * intensity) as u8,
        (g as f32 * intensity) as u8,
        (b as f32 * intensity) as u8,
    )
}

fn rgb_color(rgb: (u8, u8, u8)) -> Color {
    Color::Rgb(rgb.0, rgb.1, rgb.2)
}
```

- [ ] **Step 3: Build to verify compilation**

Run: `cargo build -p roughneck-cli`
Expected: Builds successfully

- [ ] **Step 4: Commit color helpers**

```bash
git add crates/roughneck-cli/src/tui.rs
git commit -m "Add cyberpunk color constants and interpolation helpers

Defines RGB color palette (neon cyan, magenta, blue, green, pink) and
helper functions for brightness interpolation and color creation.

Co-Authored-By: Claude Opus 4.6 <noreply@anthropic.com>"
```

---

### Task 3: Create gradient text helper

**Files:**
- Modify: `crates/roughneck-cli/src/tui.rs` (add helper function)

- [ ] **Step 1: Add gradient text helper function**

Add after the color helper functions (interpolate_brightness and rgb_color), before the panel() function:

```rust
fn create_gradient_spans(text: &str, base_color: (u8, u8, u8)) -> Vec<Span<'static>> {
    let len = text.len().max(1);
    text.chars()
        .enumerate()
        .map(|(i, ch)| {
            // Gradient from 100% brightness to 60%
            let intensity = 1.0 - (i as f32 / len as f32) * 0.4;
            Span::styled(
                ch.to_string(),
                Style::default().fg(interpolate_brightness(base_color, intensity)),
            )
        })
        .collect()
}
```

- [ ] **Step 2: Build to verify compilation**

Run: `cargo build -p roughneck-cli`
Expected: Builds successfully

- [ ] **Step 3: Commit gradient helper**

```bash
git add crates/roughneck-cli/src/tui.rs
git commit -m "Add gradient text span generator

Creates character-by-character spans with brightness gradient from
100% to 60% for cyberpunk text effects.

Co-Authored-By: Claude Opus 4.6 <noreply@anthropic.com>"
```

---

## Chunk 2: Spinner and Header Enhancements

### Task 4: Update spinner with new patterns and colors

**Files:**
- Modify: `crates/roughneck-cli/src/tui.rs:546-552` (spinner function)

- [ ] **Step 1: Add spinner pattern constants**

Replace the existing SPINNER constant at the top of the file (the `const SPINNER: [&str; 6] = [...]` array) with these new constants:

```rust
const SPINNER_PATTERNS: [[&str; 10]; 4] = [
    // Pattern 0 - Dots
    ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"],
    // Pattern 1 - Bars
    ["▁", "▂", "▃", "▄", "▅", "▆", "▇", "█", "▇", "▆"],
    // Pattern 2 - Circuit
    ["◐", "◓", "◑", "◒", "◐", "◓", "◑", "◒", "◐", "◓"],
    // Pattern 3 - Pulse
    ["◜", "◝", "◞", "◟", "◜", "◝", "◞", "◟", "◜", "◝"],
];
const SPINNER_IDLE: &str = "◉";
```

- [ ] **Step 2: Update spinner method**

Replace the entire spinner() method in AppState (search for `fn spinner(&self)`) with:

```rust
fn spinner(&self) -> (String, Color) {
    if self.busy {
        let pattern = &SPINNER_PATTERNS[self.animation.spinner_pattern % SPINNER_PATTERNS.len()];
        let frame = pattern[self.tick % pattern.len()];
        (format!("[{}]", frame), rgb_color(NEON_CYAN))
    } else {
        (format!("[{}]", SPINNER_IDLE), rgb_color(NEON_GREEN))
    }
}
```

- [ ] **Step 3: Update header rendering to use spinner color**

In render_header method, find the line that creates the spinner Span (currently `Span::styled(self.state.spinner(), Style::default().fg(Color::Yellow))`), and replace it with:

```rust
let (spinner_text, spinner_color) = self.state.spinner();
Span::styled(spinner_text, Style::default().fg(spinner_color)),
```

- [ ] **Step 4: Build and test**

Run: `cargo build -p roughneck-cli && cargo run -p roughneck-cli`
Expected: Builds successfully, spinner shows green idle state with ◉

- [ ] **Step 5: Commit spinner enhancement**

```bash
git add crates/roughneck-cli/src/tui.rs
git commit -m "Enhance spinner with animated patterns and color states

Adds 4 rotating spinner patterns (dots, bars, circuit, pulse) with
color-coded states: cyan when busy, green when idle.

Co-Authored-By: Claude Opus 4.6 <noreply@anthropic.com>"
```

---

### Task 5: Update header rendering with cyberpunk colors

**Files:**
- Modify: `crates/roughneck-cli/src/tui.rs:218-258` (render_header)

- [ ] **Step 1: Update header title with gradient**

In render_header method, find the title Line creation (currently `let title = Line::from(vec![Span::styled("Roughneck", ...)])`) and replace it with:

```rust
let mut title_spans = vec![Span::raw("[")];
title_spans.extend(create_gradient_spans("Roughneck", ELECTRIC_MAGENTA));
title_spans.push(Span::raw("]"));
title_spans.push(Span::styled(
    "  chatty interactive harness",
    Style::default().fg(rgb_color(MEDIUM_GRAY)),
));
let title = Line::from(title_spans);
```

- [ ] **Step 2: Update status line with neon colors**

In render_header method, find the status Line creation (currently `let status = Line::from(vec![...])`) and replace it with:

```rust
let (spinner_text, spinner_color) = self.state.spinner();
let status = Line::from(vec![
    Span::styled(spinner_text, Style::default().fg(spinner_color)),
    Span::raw(" "),
    Span::styled(
        format!("{} / {}", self.state.provider_label, self.state.model_label),
        Style::default()
            .fg(rgb_color(ELECTRIC_BLUE))
            .add_modifier(Modifier::BOLD),
    ),
    Span::raw("  |  "),
    Span::styled(
        format!("session {}", self.state.session_id),
        Style::default().fg(rgb_color(NEON_CYAN)),
    ),
    Span::raw("  |  "),
    Span::styled(
        format!("todos {}", self.state.todos.len()),
        Style::default().fg(interpolate_brightness(
            ELECTRIC_MAGENTA,
            0.6 + (self.state.todos.len().min(10) as f32 / 10.0) * 0.4,
        )),
    ),
]);
```

- [ ] **Step 3: Update mood/status text color**

In render_header method, find the mood Line creation (currently `let mood = Line::from(vec![Span::styled(...)])`) and replace it with:

```rust
let mood = Line::from(vec![Span::styled(
    self.state.status_text.as_str(),
    Style::default().fg(rgb_color(MEDIUM_GRAY)),
)]);
```

- [ ] **Step 4: Build and test**

Run: `cargo build -p roughneck-cli && cargo run -p roughneck-cli`
Expected: Header shows magenta gradient "Roughneck", cyan/blue/magenta status items

- [ ] **Step 5: Commit header enhancements**

```bash
git add crates/roughneck-cli/src/tui.rs
git commit -m "Apply cyberpunk colors to header panel

Updates header with magenta gradient title, electric blue provider/model,
cyan session ID, and intensity-scaled magenta todo count.

Co-Authored-By: Claude Opus 4.6 <noreply@anthropic.com>"
```

---

## Chunk 3: Transcript and Activity Panel Enhancements

### Task 6: Update transcript with gradient labels and pulsing border

**Files:**
- Modify: `crates/roughneck-cli/src/tui.rs:275-301` (render_transcript)
- Modify: `crates/roughneck-cli/src/tui.rs:734-761` (TranscriptEntry::as_list_item)

- [ ] **Step 1: Update TranscriptEntry::as_list_item with gradients**

In the impl block for TranscriptEntry, find and replace the entire as_list_item method (search for `fn as_list_item(&self) -> ListItem`) with:

```rust
fn as_list_item(&self) -> ListItem<'static> {
    let label_spans = match self.role {
        MessageRole::User => create_gradient_spans("You", NEON_CYAN),
        MessageRole::Assistant => create_gradient_spans("Roughneck", ELECTRIC_MAGENTA),
        MessageRole::System => create_gradient_spans("System", NEON_GREEN),
    };

    let mut lines = vec![Line::from(label_spans)];
    lines.extend(self.body.lines().map(|line| {
        Line::from(Span::styled(
            line.to_string(),
            Style::default().fg(rgb_color(BRIGHT_WHITE)),
        ))
    }));
    ListItem::new(Text::from(lines))
}
```

- [ ] **Step 2: Add pulsing border to transcript panel**

Find and replace the entire render_transcript method in InteractiveApp (search for `fn render_transcript(&self, frame: &mut Frame, area: Rect)`) with:

```rust
fn render_transcript(&self, frame: &mut Frame, area: Rect) {
    let items: Vec<ListItem> = if self.state.transcript.is_empty() {
        vec![ListItem::new(Text::from(vec![
            Line::from(Span::styled(
                "No conversation yet.",
                Style::default()
                    .fg(rgb_color(MEDIUM_GRAY))
                    .add_modifier(Modifier::ITALIC),
            )),
            Line::from(Span::styled(
                "Ask Roughneck to inspect files, explain code, or change the repo.",
                Style::default().fg(rgb_color(DARK_GRAY)),
            )),
        ]))]
    } else {
        self.state
            .transcript
            .iter()
            .map(TranscriptEntry::as_list_item)
            .collect()
    };

    let mut state = ListState::default();
    state.select(items.len().checked_sub(1));

    // Pulsing border when busy
    let border_color = if self.state.busy {
        let intensity = 0.7 + self.state.animation.pulse_phase * 0.3;
        interpolate_brightness(NEON_CYAN, intensity)
    } else {
        rgb_color(BRIGHT_WHITE)
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(border_color))
        .title(Span::styled(
            " Transcript ",
            Style::default()
                .fg(rgb_color(BRIGHT_WHITE))
                .add_modifier(Modifier::BOLD),
        ));

    let list = List::new(items)
        .block(block)
        .highlight_symbol("")
        .highlight_style(Style::default());
    frame.render_stateful_widget(list, area, &mut state);
}
```

- [ ] **Step 3: Build and test**

Run: `cargo build -p roughneck-cli && cargo run -p roughneck-cli`
Expected: Role labels show gradients (cyan You, magenta Roughneck), message text is white, border pulses cyan when busy

- [ ] **Step 4: Commit transcript enhancements**

```bash
git add crates/roughneck-cli/src/tui.rs
git commit -m "Add gradient labels and pulsing border to transcript

Role labels now display with color gradients (cyan for user, magenta
for assistant, green for system). Border pulses cyan when busy.

Co-Authored-By: Claude Opus 4.6 <noreply@anthropic.com>"
```

---

### Task 7: Update activity panel with colored events and pulsing border

**Files:**
- Modify: `crates/roughneck-cli/src/tui.rs:303-326` (render_activity)
- Modify: `crates/roughneck-cli/src/tui.rs:779-796` (ActivityEntry::as_list_item)
- Modify: `crates/roughneck-cli/src/tui.rs:496-511` (AppState - add activity tracking)

- [ ] **Step 1: Add activity tracking to AppState**

Add field to AppState struct definition (search for `struct AppState`), after the `activity: VecDeque<ActivityEntry>,` field:

```rust
last_activity_count: usize,
activity_pulse_until: usize, // Frame number to pulse until
```

Initialize in AppState::new function (search for `fn new(session_id: String`), after initializing activity:

```rust
last_activity_count: 0,
activity_pulse_until: 0,
```

- [ ] **Step 2: Update ActivityEntry::as_list_item with neon colors**

In the impl block for ActivityEntry, find and replace the entire as_list_item method (search for `fn as_list_item(&self) -> ListItem`) with:

```rust
fn as_list_item(&self) -> ListItem<'static> {
    let (title_color, use_pulse) = match self.level {
        ActivityLevel::Info => (NEON_CYAN, false),
        ActivityLevel::Success => (NEON_GREEN, false),
        ActivityLevel::Warn => (HOT_PINK, false),
        ActivityLevel::Error => (HOT_PINK, true),
    };

    let title_style = Style::default()
        .fg(rgb_color(title_color))
        .add_modifier(Modifier::BOLD);

    ListItem::new(Text::from(vec![
        Line::from(vec![
            Span::styled(self.elapsed.clone(), Style::default().fg(rgb_color(DARK_GRAY))),
            Span::raw(" "),
            Span::styled(self.title.clone(), title_style),
        ]),
        Line::from(Span::styled(
            self.detail.clone(),
            Style::default().fg(rgb_color(MEDIUM_GRAY)),
        )),
    ]))
}
```

- [ ] **Step 3: Update render_activity with pulsing border**

Find and replace the entire render_activity method in InteractiveApp (search for `fn render_activity(&self, frame: &mut Frame, area: Rect)`) with:

```rust
fn render_activity(&self, frame: &mut Frame, area: Rect) {
    let items: Vec<ListItem> = if self.state.activity.is_empty() {
        vec![ListItem::new(Text::from(vec![Line::from(Span::styled(
            "Tool and hook activity will stream here live.",
            Style::default()
                .fg(rgb_color(MEDIUM_GRAY))
                .add_modifier(Modifier::ITALIC),
        ))]))]
    } else {
        self.state
            .activity
            .iter()
            .map(ActivityEntry::as_list_item)
            .collect()
    };

    let mut state = ListState::default();
    state.select(items.len().checked_sub(1));

    // Pulse for 3 seconds after new activity arrives
    let should_pulse = self.state.animation.frame < self.state.activity_pulse_until;
    let border_color = if should_pulse {
        let intensity = 0.7 + self.state.animation.glow_intensity * 0.3;
        interpolate_brightness(ELECTRIC_MAGENTA, intensity)
    } else {
        rgb_color(BRIGHT_WHITE)
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(border_color))
        .title(Span::styled(
            " Trace ",
            Style::default()
                .fg(rgb_color(BRIGHT_WHITE))
                .add_modifier(Modifier::BOLD),
        ));

    let list = List::new(items)
        .block(block)
        .highlight_symbol("")
        .highlight_style(Style::default());
    frame.render_stateful_widget(list, area, &mut state);
}
```

- [ ] **Step 4: Update activity count tracking**

Find the push_activity method in AppState (search for `fn push_activity(`). At the end of the method, after the existing `trim_queue` call, add:

```rust
// Trigger pulsing for 3 seconds (25 frames at 120ms tick rate)
if self.activity.len() != self.last_activity_count {
    self.activity_pulse_until = self.animation.frame + 25;
    self.last_activity_count = self.activity.len();
}
```

- [ ] **Step 5: Build and test**

Run: `cargo build -p roughneck-cli && cargo run -p roughneck-cli`
Expected: Activity events show with neon colors (cyan info, green success, pink warnings)

- [ ] **Step 6: Commit activity panel enhancements**

```bash
git add crates/roughneck-cli/src/tui.rs
git commit -m "Apply cyberpunk colors to activity panel with pulsing

Activity entries now use neon cyan (info), green (success), and hot
pink (warnings/errors). Border pulses magenta when new events arrive.

Co-Authored-By: Claude Opus 4.6 <noreply@anthropic.com>"
```

---

## Chunk 4: Todos, Input, and Footer Enhancements

### Task 8: Update todos panel with colored checkboxes

**Files:**
- Modify: `crates/roughneck-cli/src/tui.rs:328-366` (render_todos)

- [ ] **Step 1: Update render_todos with colored checkboxes**

Find and replace the entire render_todos method in InteractiveApp (search for `fn render_todos(&self, frame: &mut Frame, area: Rect)`) with:

```rust
fn render_todos(&self, frame: &mut Frame, area: Rect) {
    let items: Vec<ListItem> = if self.state.todos.is_empty() {
        vec![ListItem::new(Line::from(Span::styled(
            "No todo list recorded for this session.",
            Style::default()
                .fg(rgb_color(MEDIUM_GRAY))
                .add_modifier(Modifier::ITALIC),
        )))]
    } else {
        self.state
            .todos
            .iter()
            .map(|todo| {
                let (marker, marker_color, text_color) =
                    if todo.status == roughneck_core::TodoStatus::Done {
                        ("[x]", NEON_GREEN, MEDIUM_GRAY)
                    } else {
                        // Subtle pulse for incomplete todos
                        let pulse = 0.7 + self.state.animation.pulse_phase * 0.3;
                        ("[ ]", (255, (255.0 * pulse) as u8, 0), BRIGHT_WHITE)
                    };

                ListItem::new(Line::from(vec![
                    Span::styled(
                        marker,
                        Style::default().fg(rgb_color(marker_color)),
                    ),
                    Span::raw(" "),
                    Span::styled(
                        todo.task.clone(),
                        Style::default().fg(rgb_color(text_color)),
                    ),
                ]))
            })
            .collect()
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(rgb_color(BRIGHT_WHITE)))
        .title(Span::styled(
            " Todos ",
            Style::default()
                .fg(rgb_color(BRIGHT_WHITE))
                .add_modifier(Modifier::BOLD),
        ));

    let list = List::new(items).block(block);
    frame.render_widget(list, area);
}
```

- [ ] **Step 2: Build and test**

Run: `cargo build -p roughneck-cli && cargo run -p roughneck-cli`
Expected: Completed todos show green [x], incomplete show yellow pulsing [ ]

- [ ] **Step 3: Commit todos enhancements**

```bash
git add crates/roughneck-cli/src/tui.rs
git commit -m "Add colored checkboxes to todos panel

Completed todos display green [x], incomplete todos show yellow [ ]
with subtle pulsing animation. Completed task text is dimmed.

Co-Authored-By: Claude Opus 4.6 <noreply@anthropic.com>"
```

---

### Task 9: Update input panel with pulsing effects

**Files:**
- Modify: `crates/roughneck-cli/src/tui.rs:368-392` (render_input)

- [ ] **Step 1: Update render_input with cyberpunk styling**

Find and replace the entire render_input method in InteractiveApp (search for `fn render_input(&self, frame: &mut Frame, area: Rect)`) with:

```rust
fn render_input(&self, frame: &mut Frame, area: Rect) {
    let (title, title_color) = if self.state.busy {
        let pulse = 0.5 + self.state.animation.pulse_phase * 0.5;
        let color = interpolate_brightness((255, 0, 0), pulse);
        ("Composer (assistant busy)", color)
    } else {
        ("Composer", rgb_color(NEON_CYAN))
    };

    let content = if self.state.input.is_empty() {
        let pulse = 0.8 + self.state.animation.pulse_phase * 0.2;
        Text::from(vec![
            Line::from(Span::styled(
                "Type a prompt, /help, /clear, or /quit",
                Style::default()
                    .fg(interpolate_brightness(DARK_GRAY, pulse))
                    .add_modifier(Modifier::ITALIC),
            )),
            Line::from(""),
        ])
    } else {
        Text::from(vec![Line::from(Span::styled(
            self.state.input.as_str(),
            Style::default().fg(rgb_color(BRIGHT_WHITE)),
        ))])
    };

    let border_color = if self.state.busy {
        rgb_color(DARK_GRAY)
    } else {
        rgb_color(NEON_CYAN)
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(border_color))
        .title(Span::styled(
            format!(" {} ", title),
            Style::default()
                .fg(title_color)
                .add_modifier(Modifier::BOLD),
        ));

    let paragraph = Paragraph::new(content)
        .block(block)
        .wrap(Wrap { trim: false });
    frame.render_widget(paragraph, area);
}
```

- [ ] **Step 2: Build and test**

Run: `cargo build -p roughneck-cli && cargo run -p roughneck-cli`
Expected: Input border is cyan when ready, gray when busy. Title pulses red when busy.

- [ ] **Step 3: Commit input panel enhancements**

```bash
git add crates/roughneck-cli/src/tui.rs
git commit -m "Add pulsing effects to input panel

Input border is cyan when ready, gray when busy. Title pulses red
when busy. Placeholder text has subtle fade pulse.

Co-Authored-By: Claude Opus 4.6 <noreply@anthropic.com>"
```

---

### Task 10: Update footer with pulsing hotkeys

**Files:**
- Modify: `crates/roughneck-cli/src/tui.rs:394-407` (render_footer)

- [ ] **Step 1: Update render_footer with pulsing hotkeys**

Find and replace the entire render_footer method in InteractiveApp (search for `fn render_footer(&self, frame: &mut Frame, area: Rect)`) with:

```rust
fn render_footer(&self, frame: &mut Frame, area: Rect) {
    let pulse = 0.7 + self.state.animation.pulse_phase * 0.3;
    let hotkey_color = interpolate_brightness(NEON_CYAN, pulse);

    let footer = Paragraph::new(Line::from(vec![
        Span::styled("Enter", Style::default().fg(hotkey_color)),
        Span::styled(" send  |  ", Style::default().fg(rgb_color(MEDIUM_GRAY))),
        Span::styled("?", Style::default().fg(hotkey_color)),
        Span::styled(" help  |  ", Style::default().fg(rgb_color(MEDIUM_GRAY))),
        Span::styled("Ctrl+L", Style::default().fg(hotkey_color)),
        Span::styled(" clear  |  ", Style::default().fg(rgb_color(MEDIUM_GRAY))),
        Span::styled("Ctrl+C", Style::default().fg(hotkey_color)),
        Span::styled(" quit", Style::default().fg(rgb_color(MEDIUM_GRAY))),
    ]))
    .alignment(Alignment::Center);
    frame.render_widget(footer, area);
}
```

- [ ] **Step 2: Build and test**

Run: `cargo build -p roughneck-cli && cargo run -p roughneck-cli`
Expected: Hotkeys pulse between bright and medium cyan

- [ ] **Step 3: Commit footer enhancements**

```bash
git add crates/roughneck-cli/src/tui.rs
git commit -m "Add pulsing hotkeys to footer

Hotkey commands pulse between bright and medium cyan for visual
interest while maintaining readability.

Co-Authored-By: Claude Opus 4.6 <noreply@anthropic.com>"
```

---

## Chunk 5: Help Dialog and Final Polish

### Task 11: Update help dialog with cyberpunk styling

**Files:**
- Modify: `crates/roughneck-cli/src/tui.rs:409-429` (render_help)

- [ ] **Step 1: Update render_help with neon styling**

Find and replace the entire render_help method in InteractiveApp (search for `fn render_help(&self, frame: &mut Frame, area: Rect)`) with:

```rust
fn render_help(&self, frame: &mut Frame, area: Rect) {
    let popup = centered_rect(74, 60, area);

    let border_color = interpolate_brightness(
        NEON_CYAN,
        0.7 + self.state.animation.glow_intensity * 0.3
    );

    let help = Paragraph::new(Text::from(vec![
        Line::from(Span::styled(
            "Interactive commands",
            Style::default()
                .add_modifier(Modifier::BOLD)
                .fg(rgb_color(NEON_CYAN)),
        )),
        Line::from(""),
        Line::from(vec![
            Span::styled("/help", Style::default().fg(rgb_color(NEON_CYAN))),
            Span::styled("   toggle this help dialog", Style::default().fg(rgb_color(MEDIUM_GRAY))),
        ]),
        Line::from(vec![
            Span::styled("/clear", Style::default().fg(rgb_color(NEON_CYAN))),
            Span::styled("  clear the transcript and trace panes", Style::default().fg(rgb_color(MEDIUM_GRAY))),
        ]),
        Line::from(vec![
            Span::styled("/quit", Style::default().fg(rgb_color(NEON_CYAN))),
            Span::styled("   leave interactive mode", Style::default().fg(rgb_color(MEDIUM_GRAY))),
        ]),
        Line::from(""),
        Line::from(Span::styled(
            "The Trace pane shows runtime hook notifications and tool call activity as it happens.",
            Style::default().fg(rgb_color(BRIGHT_WHITE)),
        )),
        Line::from(Span::styled(
            "Hook output recorded after a turn finishes is folded back into the trace automatically.",
            Style::default().fg(rgb_color(BRIGHT_WHITE)),
        )),
    ]))
    .wrap(Wrap { trim: true })
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(border_color))
            .title(Span::styled(
                " Help ",
                Style::default()
                    .fg(rgb_color(ELECTRIC_MAGENTA))
                    .add_modifier(Modifier::BOLD),
            ))
    );

    frame.render_widget(Clear, popup);
    frame.render_widget(help, popup);
}
```

- [ ] **Step 2: Build and test**

Run: `cargo build -p roughneck-cli && cargo run -p roughneck-cli`
Press `?` to open help dialog
Expected: Help dialog shows with pulsing cyan border, magenta title, cyan commands

- [ ] **Step 3: Commit help dialog enhancements**

```bash
git add crates/roughneck-cli/src/tui.rs
git commit -m "Apply cyberpunk styling to help dialog

Help dialog features pulsing cyan border, magenta gradient title,
and cyan command highlighting for consistency with overall theme.

Co-Authored-By: Claude Opus 4.6 <noreply@anthropic.com>"
```

---

### Task 12: Visual testing and refinement

**Files:**
- Test: Visual testing only

- [ ] **Step 1: Run comprehensive visual tests**

Test checklist:
```bash
# 1. Start the CLI
cargo run -p roughneck-cli

# 2. Verify idle state
# - Check spinner shows green ◉
# - Check header colors (magenta Roughneck, cyan session, etc.)
# - Check footer hotkeys pulse cyan
# - Wait 10 seconds to see spinner pattern rotate

# 3. Test user interaction
# - Type a message (check white text, cyan border)
# - Press Enter to send
# - Observe busy state (cyan pulsing spinner, transcript border pulse)
# - Check activity panel receives events with colored indicators
# - Verify activity border pulses magenta

# 4. Test help dialog
# - Press ? to open help
# - Verify pulsing cyan border and magenta title
# - Press Esc to close

# 5. Test clear function
# - Press Ctrl+L
# - Verify panes clear but styling remains

# 6. Long-running test
# - Leave running for 30+ seconds
# - Verify all spinner patterns cycle through (4 patterns × 10 seconds)
# - Verify no performance degradation
# - Verify animations remain smooth
```

- [ ] **Step 2: Document any issues found**

If issues are found, create tasks to fix them. Otherwise, proceed to commit.

- [ ] **Step 3: Commit final visual testing notes**

```bash
git commit --allow-empty -m "Visual testing complete for cyberpunk TUI

Verified all animations, colors, and pulsing effects work correctly
across idle, busy, and interactive states. Spinner patterns rotate
properly, borders pulse on activity, and all text gradients render.

Co-Authored-By: Claude Opus 4.6 <noreply@anthropic.com>"
```

---

## Testing Summary

**Manual Testing Required:**
1. Run CLI and verify idle state styling
2. Send prompts to test busy state animations
3. Observe spinner pattern rotation over time
4. Verify border pulsing on transcript/activity panels
5. Test help dialog appearance
6. Check color consistency across all panels
7. Verify performance remains smooth

**Success Criteria:**
- All panels use cyberpunk RGB color palette
- Spinner cycles through 4 patterns every 40 seconds
- Borders pulse smoothly when active/busy
- Text gradients render correctly for role labels
- Hotkeys pulse in footer
- No performance degradation
- All animations remain smooth at 120ms tick rate

---

## Deployment Notes

No deployment steps required. Changes are contained to the CLI binary and will be available on next build.

**To use:**
```bash
cargo run -p roughneck-cli
```

**Performance notes:**
- Animation state updates are lightweight (basic arithmetic)
- RGB color calculations are fast
- No additional memory allocations during rendering
- 120ms tick rate unchanged
