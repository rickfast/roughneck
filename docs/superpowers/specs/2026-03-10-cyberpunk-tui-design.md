# Cyberpunk Neon TUI Enhancement Design

## Overview

Enhance the existing Ratatui-based TUI in `roughneck-cli` with cyberpunk aesthetics: vibrant neon colors, smooth animations, and pulsing effects. The goal is medium-intensity visual enhancement that remains professional while adding engaging visual feedback.

## Design Constraints

- No new dependencies - use existing ratatui capabilities
- Keep current layout and panel structure
- Maintain performance with 120ms tick rate
- Build on existing AppState and rendering architecture
- Professional and usable, not distracting

## Color Palette

### Primary Colors (RGB)
- **Neon Cyan**: `#00FFFF` - primary accent, active elements, user interactions
- **Electric Magenta**: `#FF00FF` - secondary accent, important information
- **Electric Blue**: `#0080FF` - metadata, links, secondary information
- **Neon Green**: `#00FF88` - success states, completed items
- **Hot Pink**: `#FF1493` - warnings, highlights

### Background/Neutral
- **Deep Black**: `#000000` - base background
- **Dark Gray**: `#1A1A1A` - panel backgrounds
- **Medium Gray**: `#808080` - muted text, timestamps
- **Bright White**: `#FFFFFF` - primary text, high readability

## Animation Architecture

### Animation State

Add `AnimationState` struct to `AppState`:

```rust
struct AnimationState {
    frame: usize,           // Master frame counter (increments every tick)
    spinner_pattern: usize, // Current spinner pattern (0-3)
    pulse_phase: f32,       // 0.0-1.0 for smooth pulsing
    glow_intensity: f32,    // 0.0-1.0 for border glow
    color_cycle: usize,     // For rainbow/cycling effects
}
```

### Animation Timing

- **Tick rate**: 120ms (existing constant)
- **Spinner speed**: Change frame every tick
- **Pulse cycle**: Complete cycle every ~2 seconds (16-20 ticks)
- **Glow cycle**: Complete cycle every ~3 seconds (24-30 ticks)
- **Pattern rotation**: Switch spinner patterns every 10 seconds

### Frame Update Logic

On each `on_tick()` call:
1. Increment master frame counter
2. Update pulse_phase: `(frame % 16) as f32 / 16.0`
3. Update glow_intensity: `(frame % 24) as f32 / 24.0`
4. Update spinner_pattern: `(frame / 80) % 4`

## Animation Effects

### 1. Enhanced Spinner (Header)

**Current**: Simple dot array `["[.]", "[..]", "[...]", "[..]", "[.]", "[ ]"]`

**Enhanced**: Four rotating patterns with color states

- **Pattern 0 - Dots**: `⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏`
- **Pattern 1 - Bars**: `▁▂▃▄▅▆▇█▇▆▅▄▃▂`
- **Pattern 2 - Circuit**: `◐◓◑◒`
- **Pattern 3 - Pulse**: `◜◝◞◟`

**Color coding**:
- Busy: Neon cyan
- Idle: Neon green
- Error: Hot pink

### 2. Pulsing Borders

Borders oscillate in brightness when panels are active:

- **Transcript panel**: Pulses cyan when assistant is responding (`state.busy`)
- **Activity panel**: Pulses magenta when new events arrive (track last event count)
- **Input panel**: Bright cyan (active) or dim gray (busy)

**Implementation**: Interpolate RGB brightness between 70% and 100% based on pulse_phase

### 3. Gradient Text

Multi-span text with varying brightness for visual depth:

- **Role labels**:
  - "You": Bright cyan → Dim cyan (3 chars: 100%, 80%, 60%)
  - "Roughneck": Bright magenta → Dim magenta (9 chars: gradient)
  - "System": Bright green → Dim green (6 chars: gradient)

- **Header title**: "Roughneck" uses cyan/magenta alternating gradient

**Implementation**: Create character-by-character spans with calculated brightness

### 4. Status Indicators

- **Footer hotkeys**: Pulse between bright and medium cyan (70%-100%)
- **Busy indicator**: "assistant busy" flashes slowly (1.5s cycle)
- **Todo count**: Magenta intensity scales with count (more todos = brighter)

## Panel-by-Panel Visual Design

### Header Panel (4 lines)

**Line 1 - Title**:
```
[Roughneck]  chatty interactive harness
```
- "Roughneck": Black text on cyan/magenta gradient background, bold
- Subtitle: Medium gray, regular

**Line 2 - Status**:
```
[◐] anthropic / claude-sonnet-4  |  session cli  |  todos 3
```
- Spinner: Animated pattern with color state
- Provider/model: Electric blue, subtle pulse when busy
- Session ID: Bright cyan
- Todo count: Magenta with brightness proportional to count

**Line 3 - Status text**:
```
dispatching prompt (123 chars)
```
- Dynamic status: Medium gray normally, bright cyan when active

**Border**: Rounded, white

### Transcript Panel (main area, left)

**Role labels**:
- "You": Cyan gradient, bold
- "Roughneck": Magenta gradient, bold
- "System": Green gradient, bold

**Message text**: Bright white for maximum readability

**Border**:
- Normal: White rounded
- Busy: Pulsing cyan (brightness 70%-100%)

### Activity/Trace Panel (top right)

**Timestamps**: `+MM:SS` - Dark gray

**Activity entries**:
- Info: Cyan title, medium gray detail
- Success: Neon green title, medium gray detail
- Warn: Hot pink title, medium gray detail
- Error: Red title with intensity pulse, white detail

**Border**:
- Normal: White rounded
- Active (new events): Pulsing magenta

### Todos Panel (bottom right)

**Checkboxes**:
- `[ ]`: Yellow with subtle pulse (incomplete)
- `[x]`: Neon green, solid (complete)

**Task text**:
- Incomplete: Bright white
- Complete: Medium gray

**Border**: White rounded

### Input/Composer Panel

**Title**:
- "Composer": Bright cyan when ready
- "Composer (assistant busy)": Dim text with red pulse when busy

**Placeholder**:
```
Type a prompt, /help, /clear, or /quit
```
- Dark gray, italic, slow fade pulse (80%-100% brightness)

**User input**: Bright white

**Border**:
- Active: Bright cyan
- Busy: Dim gray

### Footer (1 line)

**Hotkeys**: Center-aligned
```
Enter send  |  ? help  |  Ctrl+L clear  |  Ctrl+C quit
```
- Command keys: Pulsing cyan (70%-100%)
- Labels: Medium gray
- Separators: Dark gray

### Help Dialog (popup overlay)

**Border**: Bright cyan with glow effect (pulsing)

**Title**: "Help" in magenta gradient

**Content**:
- Section headers: Bright cyan, bold
- Commands: White
- Descriptions: Medium gray

## Technical Implementation Notes

### RGB Color Support

Ratatui supports RGB via `Color::Rgb(r, g, b)`:
```rust
Style::default().fg(Color::Rgb(0, 255, 255)) // Neon cyan
```

### Brightness Interpolation

For pulsing/gradients:
```rust
fn interpolate_brightness(base_rgb: (u8, u8, u8), intensity: f32) -> Color {
    let (r, g, b) = base_rgb;
    Color::Rgb(
        (r as f32 * intensity) as u8,
        (g as f32 * intensity) as u8,
        (b as f32 * intensity) as u8,
    )
}
```

### Gradient Text

Create spans with calculated brightness:
```rust
fn create_gradient(text: &str, color: (u8, u8, u8)) -> Vec<Span> {
    text.chars().enumerate().map(|(i, ch)| {
        let intensity = 1.0 - (i as f32 / text.len() as f32) * 0.4; // 100% to 60%
        Span::styled(
            ch.to_string(),
            Style::default().fg(interpolate_brightness(color, intensity))
        )
    }).collect()
}
```

### Animation State Updates

In `AppState::on_tick()`:
```rust
fn on_tick(&mut self) {
    self.tick = self.tick.wrapping_add(1);

    // Update animation state
    self.animation.frame = self.tick;
    self.animation.pulse_phase = ((self.tick % 16) as f32) / 16.0;
    self.animation.glow_intensity = (((self.tick % 24) as f32) / 24.0 * 2.0 * PI).sin().abs();
    self.animation.spinner_pattern = (self.tick / 80) % 4;
}
```

### Event-Triggered Animations

Track state changes to trigger animations:
- When `state.busy` transitions to `true`: Reset pulse phase for border animation
- When new activity event arrives: Trigger activity panel border pulse
- When todo count changes: Update intensity calculation

## Performance Considerations

- All animations computed per-frame, no background threads
- Interpolation math is lightweight (basic arithmetic)
- No additional allocations during animation updates
- 120ms tick rate unchanged
- RGB colors supported by all modern terminals

## Testing Approach

1. Visual inspection in terminal
2. Test with different terminal emulators (iTerm2, Terminal.app, Alacritty)
3. Verify performance remains smooth
4. Test with long-running sessions (spinner pattern rotation)
5. Test activity bursts (border pulsing)

## Future Enhancements (Out of Scope)

- Configurable color schemes
- Animation intensity settings
- Custom spinner patterns
- Smooth scrolling effects
- Progress bars with gradients
