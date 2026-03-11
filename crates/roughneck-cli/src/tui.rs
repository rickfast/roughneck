use anyhow::{Context, Result};
use async_trait::async_trait;
use crossterm::ExecutableCommand;
use crossterm::event::{self, Event as CEvent, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Alignment, Constraint, Direction, Flex, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{
    Block, BorderType, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap,
};
use ratatui::{Frame, Terminal};
use roughneck_core::{
    ChatMessage, DeepAgentConfig, HookOutputSummary, Result as CoreResult, Role,
    SessionInvokeRequest, SessionInvokeResponse, TodoItem,
};
use roughneck_runtime::{
    AgentSession, DeepAgent, HookDecision, HookEvent, HookExecutor, HookPayload,
};
use serde_json::Value;
use std::borrow::Cow;
use std::collections::VecDeque;
use std::io::{self, Stdout};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};

const MAX_TRANSCRIPT_ITEMS: usize = 128;
const MAX_ACTIVITY_ITEMS: usize = 256;
const TICK_RATE: Duration = Duration::from_millis(120);
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

// Cyberpunk neon color palette
const NEON_CYAN: (u8, u8, u8) = (0, 255, 255);
const ELECTRIC_MAGENTA: (u8, u8, u8) = (255, 0, 255);
const ELECTRIC_BLUE: (u8, u8, u8) = (0, 128, 255);
const NEON_GREEN: (u8, u8, u8) = (0, 255, 136);
const HOT_PINK: (u8, u8, u8) = (255, 20, 147);
const BRIGHT_WHITE: (u8, u8, u8) = (255, 255, 255);
const MEDIUM_GRAY: (u8, u8, u8) = (128, 128, 128);
const DARK_GRAY: (u8, u8, u8) = (64, 64, 64);

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

pub(crate) struct InteractiveApp {
    session: AgentSession,
    runtime_tx: UnboundedSender<RuntimeEvent>,
    runtime_rx: UnboundedReceiver<RuntimeEvent>,
    state: AppState,
}

impl InteractiveApp {
    pub(crate) async fn new(
        config: DeepAgentConfig,
        provider_label: String,
        model_label: String,
    ) -> Result<Self> {
        let (runtime_tx, runtime_rx) = mpsc::unbounded_channel();
        let hook_executor = Arc::new(TuiHookExecutor::new(runtime_tx.clone()));
        let agent = DeepAgent::new(config)
            .context("failed to initialize deep agent")?
            .with_hook_executor(hook_executor);
        let session = agent
            .start_session(roughneck_core::SessionInit {
                session_id: Some("cli".to_string()),
                ..roughneck_core::SessionInit::default()
            })
            .await
            .context("failed to start session")?;
        let state = AppState::new(
            session.session_id().to_string(),
            provider_label,
            model_label,
        );

        Ok(Self {
            session,
            runtime_tx,
            runtime_rx,
            state,
        })
    }

    pub(crate) async fn run(mut self) -> Result<()> {
        let mut terminal = TuiTerminal::new()?;

        loop {
            self.drain_runtime_events();
            self.state.on_tick();

            terminal.draw(|frame| self.render(frame))?;

            if self.state.should_quit {
                break;
            }

            if !event::poll(TICK_RATE).context("failed to poll terminal events")? {
                continue;
            }

            match event::read().context("failed to read terminal event")? {
                CEvent::Key(key) if key.kind == KeyEventKind::Press => self.handle_key(key),
                CEvent::Paste(text) => self.state.input.push_str(&text),
                _ => {}
            }
        }

        Ok(())
    }

    fn drain_runtime_events(&mut self) {
        while let Ok(event) = self.runtime_rx.try_recv() {
            self.state.handle_runtime_event(event);
        }
    }

    fn handle_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.state.should_quit = true;
            }
            KeyCode::Char('l') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.state.reset_view();
            }
            KeyCode::Esc => {
                if self.state.show_help {
                    self.state.show_help = false;
                } else {
                    self.state.input.clear();
                }
            }
            KeyCode::Char('?') => {
                self.state.show_help = !self.state.show_help;
            }
            KeyCode::Enter => self.submit_input(),
            KeyCode::Backspace => {
                self.state.input.pop();
            }
            KeyCode::Char(ch) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.state.input.push(ch);
            }
            _ => {}
        }
    }

    fn submit_input(&mut self) {
        let input = self.state.input.trim().to_string();
        self.state.input.clear();

        if input.is_empty() {
            return;
        }

        match input.as_str() {
            "/quit" | "/exit" => {
                self.state.should_quit = true;
                return;
            }
            "/clear" => {
                self.state.reset_view();
                return;
            }
            "/help" => {
                self.state.show_help = !self.state.show_help;
                return;
            }
            _ => {}
        }

        if self.state.busy {
            self.state.push_activity(
                ActivityLevel::Warn,
                "invoke busy",
                "wait for the current turn to finish before sending another prompt",
            );
            return;
        }

        self.state.push_message(MessageRole::User, input.clone());
        self.state.busy = true;
        self.state.status_text = format!("dispatching prompt ({} chars)", input.len());
        self.state.push_activity(
            ActivityLevel::Info,
            "prompt queued",
            truncate_text(&input, 120),
        );

        let session = self.session.clone();
        let runtime_tx = self.runtime_tx.clone();
        tokio::spawn(async move {
            let result = session
                .invoke(SessionInvokeRequest {
                    messages: vec![ChatMessage::user(input)],
                })
                .await;

            let event = match result {
                Ok(response) => RuntimeEvent::Response(response),
                Err(err) => RuntimeEvent::Error(err.to_string()),
            };
            let _ = runtime_tx.send(event);
        });
    }

    fn render(&self, frame: &mut Frame) {
        let area = frame.area();
        let layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(4),
                Constraint::Min(12),
                Constraint::Length(5),
                Constraint::Length(1),
            ])
            .split(area);

        self.render_header(frame, layout[0]);
        self.render_body(frame, layout[1]);
        self.render_input(frame, layout[2]);
        self.render_footer(frame, layout[3]);

        if self.state.show_help {
            self.render_help(frame, area);
        }
    }

    fn render_header(&self, frame: &mut Frame, area: Rect) {
        let mut title_spans = vec![Span::raw("[")];
        title_spans.extend(create_gradient_spans("Roughneck", ELECTRIC_MAGENTA));
        title_spans.push(Span::raw("]"));
        title_spans.push(Span::styled(
            "  chatty interactive harness",
            Style::default().fg(rgb_color(MEDIUM_GRAY)),
        ));
        let title = Line::from(title_spans);
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
        let mood = Line::from(vec![Span::styled(
            self.state.status_text.as_str(),
            Style::default().fg(rgb_color(MEDIUM_GRAY)),
        )]);

        let paragraph = Paragraph::new(Text::from(vec![title, status, mood]))
            .block(panel("Session"))
            .wrap(Wrap { trim: true });
        frame.render_widget(paragraph, area);
    }

    fn render_body(&self, frame: &mut Frame, area: Rect) {
        let columns = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Min(40), Constraint::Length(42)])
            .split(area);
        let sidebar = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(10), Constraint::Length(10)])
            .split(columns[1]);

        self.render_transcript(frame, columns[0]);
        self.render_activity(frame, sidebar[0]);
        self.render_todos(frame, sidebar[1]);
    }

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

        // Pulsing border for 3 seconds after new activity
        let border_color = if self.state.animation.frame < self.state.activity_pulse_until {
            let intensity = 0.7 + self.state.animation.pulse_phase * 0.3;
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
                        Span::styled(marker, Style::default().fg(rgb_color(marker_color))),
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

    fn render_help(&self, frame: &mut Frame, area: Rect) {
        let popup = centered_rect(74, 60, area);

        // Pulsing cyan border (70%-100% intensity)
        let border_intensity = 0.7 + self.state.animation.glow_intensity * 0.3;
        let border_color = interpolate_brightness(NEON_CYAN, border_intensity);

        let help = Paragraph::new(Text::from(vec![
            Line::from(Span::styled(
                "Interactive commands",
                Style::default()
                    .add_modifier(Modifier::BOLD)
                    .fg(rgb_color(ELECTRIC_MAGENTA)),
            )),
            Line::from(""),
            Line::from(vec![
                Span::styled("/help", Style::default().fg(rgb_color(NEON_CYAN))),
                Span::styled("   toggle this help dialog", Style::default().fg(rgb_color(BRIGHT_WHITE))),
            ]),
            Line::from(vec![
                Span::styled("/clear", Style::default().fg(rgb_color(NEON_CYAN))),
                Span::styled("  clear the transcript and trace panes", Style::default().fg(rgb_color(BRIGHT_WHITE))),
            ]),
            Line::from(vec![
                Span::styled("/quit", Style::default().fg(rgb_color(NEON_CYAN))),
                Span::styled("   leave interactive mode", Style::default().fg(rgb_color(BRIGHT_WHITE))),
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
}

#[derive(Debug)]
struct TuiTerminal {
    terminal: Terminal<CrosstermBackend<Stdout>>,
}

impl TuiTerminal {
    fn new() -> Result<Self> {
        enable_raw_mode().context("failed to enable raw mode")?;
        let mut stdout = io::stdout();
        stdout
            .execute(EnterAlternateScreen)
            .context("failed to enter alternate screen")?;
        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend).context("failed to initialize terminal")?;
        terminal.clear().context("failed to clear terminal")?;
        Ok(Self { terminal })
    }

    fn draw(&mut self, render: impl FnOnce(&mut Frame)) -> Result<()> {
        self.terminal
            .draw(render)
            .context("failed to render frame")?;
        Ok(())
    }
}

impl Drop for TuiTerminal {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = io::stdout().execute(LeaveAlternateScreen);
        let _ = self.terminal.show_cursor();
    }
}

#[derive(Debug)]
struct TuiHookExecutor {
    runtime_tx: UnboundedSender<RuntimeEvent>,
}

impl TuiHookExecutor {
    fn new(runtime_tx: UnboundedSender<RuntimeEvent>) -> Self {
        Self { runtime_tx }
    }
}

#[async_trait]
impl HookExecutor for TuiHookExecutor {
    fn has_handlers(&self) -> bool {
        true
    }

    async fn execute(&self, _event: HookEvent, payload: HookPayload) -> CoreResult<HookDecision> {
        let _ = self.runtime_tx.send(RuntimeEvent::Hook(payload));
        Ok(HookDecision::default())
    }
}

#[derive(Debug)]
enum RuntimeEvent {
    Hook(HookPayload),
    Response(SessionInvokeResponse),
    Error(String),
}

#[derive(Debug)]
struct AppState {
    session_id: String,
    provider_label: String,
    model_label: String,
    input: String,
    transcript: VecDeque<TranscriptEntry>,
    activity: VecDeque<ActivityEntry>,
    todos: Vec<TodoItem>,
    busy: bool,
    should_quit: bool,
    show_help: bool,
    status_text: String,
    started_at: Instant,
    tick: usize,
    animation: AnimationState,
    last_activity_count: usize,
    activity_pulse_until: usize, // Frame number to pulse until
}

impl AppState {
    fn new(session_id: String, provider_label: String, model_label: String) -> Self {
        let mut state = Self {
            session_id,
            provider_label,
            model_label,
            input: String::new(),
            transcript: VecDeque::new(),
            activity: VecDeque::new(),
            todos: Vec::new(),
            busy: false,
            should_quit: false,
            show_help: false,
            status_text: "ready for the next prompt".to_string(),
            started_at: Instant::now(),
            tick: 0,
            animation: AnimationState::new(),
            last_activity_count: 0,
            activity_pulse_until: 0,
        };
        state.push_message(
            MessageRole::Assistant,
            "Roughneck is online. Ask for repo changes, explanations, or file operations.",
        );
        state.push_activity(
            ActivityLevel::Info,
            "session ready",
            "live hook and tool trace will stream here during each turn",
        );
        state
    }

    fn on_tick(&mut self) {
        self.tick = self.tick.wrapping_add(1);

        // Update animation state
        self.animation.frame = self.tick;
        self.animation.pulse_phase = ((self.tick % 16) as f32) / 16.0;
        let glow_angle = ((self.tick % 24) as f32) / 24.0 * 2.0 * std::f32::consts::PI;
        self.animation.glow_intensity = glow_angle.sin().abs();
        self.animation.spinner_pattern = (self.tick / 80) % 4;
    }

    fn spinner(&self) -> (String, Color) {
        if self.busy {
            let pattern =
                &SPINNER_PATTERNS[self.animation.spinner_pattern % SPINNER_PATTERNS.len()];
            let frame = pattern[self.tick % pattern.len()];
            (format!("[{}]", frame), rgb_color(NEON_CYAN))
        } else {
            (format!("[{}]", SPINNER_IDLE), rgb_color(NEON_GREEN))
        }
    }

    fn reset_view(&mut self) {
        self.transcript.clear();
        self.activity.clear();
        self.todos.clear();
        self.busy = false;
        self.show_help = false;
        self.status_text = "cleared local view; session history remains in memory".to_string();
        self.push_message(
            MessageRole::Assistant,
            "Local panes cleared. The session itself is still alive, so the model remembers prior turns.",
        );
        self.push_activity(
            ActivityLevel::Info,
            "view reset",
            "local transcript and trace panes were cleared",
        );
    }

    fn push_message(&mut self, role: MessageRole, body: impl Into<String>) {
        self.transcript.push_back(TranscriptEntry {
            role,
            body: body.into(),
        });
        trim_queue(&mut self.transcript, MAX_TRANSCRIPT_ITEMS);
    }

    fn push_activity(
        &mut self,
        level: ActivityLevel,
        title: impl Into<String>,
        detail: impl Into<String>,
    ) {
        self.activity.push_back(ActivityEntry {
            elapsed: format_elapsed(self.started_at.elapsed()),
            level,
            title: title.into(),
            detail: detail.into(),
        });
        trim_queue(&mut self.activity, MAX_ACTIVITY_ITEMS);

        // Trigger pulsing for 3 seconds (25 frames at 120ms tick rate)
        if self.activity.len() != self.last_activity_count {
            self.activity_pulse_until = self.animation.frame + 25;
            self.last_activity_count = self.activity.len();
        }
    }

    fn handle_runtime_event(&mut self, event: RuntimeEvent) {
        match event {
            RuntimeEvent::Hook(payload) => self.handle_hook(payload),
            RuntimeEvent::Response(response) => self.handle_response(response),
            RuntimeEvent::Error(error) => {
                self.busy = false;
                self.status_text = "invoke failed".to_string();
                self.push_activity(ActivityLevel::Error, "invoke failed", error.clone());
                self.push_message(MessageRole::System, format!("invoke failed: {error}"));
            }
        }
    }

    fn handle_hook(&mut self, payload: HookPayload) {
        let event_name = payload.hook_event_name.clone();
        match event_name.as_str() {
            "Notification" => {
                let label = payload
                    .message
                    .unwrap_or_else(|| "notification".to_string());
                let detail = payload.tool_input.as_ref().map_or_else(
                    || "runtime notification".to_string(),
                    |value| summarize_value(value, 140),
                );
                self.status_text = label.clone();
                self.push_activity(ActivityLevel::Info, label, detail);
            }
            "PreToolUse" => {
                let tool_name = payload
                    .tool_name
                    .unwrap_or_else(|| "unknown_tool".to_string());
                let tool_call = short_call_id(payload.tool_call_id.as_deref());
                let detail = payload.tool_input.as_ref().map_or_else(
                    || "waiting for tool input".to_string(),
                    |value| summarize_value(value, 140),
                );
                self.status_text = format!("running {tool_name}");
                self.push_activity(
                    ActivityLevel::Info,
                    format!("tool {tool_name} started {tool_call}"),
                    detail,
                );
            }
            "PostToolUse" => {
                let tool_name = payload
                    .tool_name
                    .unwrap_or_else(|| "unknown_tool".to_string());
                let tool_call = short_call_id(payload.tool_call_id.as_deref());
                if let Some(error) = payload.tool_error {
                    self.push_activity(
                        ActivityLevel::Error,
                        format!("tool {tool_name} failed {tool_call}"),
                        error,
                    );
                } else {
                    let detail = payload.tool_response.as_ref().map_or_else(
                        || "tool completed without a response payload".to_string(),
                        |value| summarize_value(value, 140),
                    );
                    self.push_activity(
                        ActivityLevel::Success,
                        format!("tool {tool_name} finished {tool_call}"),
                        detail,
                    );
                }
            }
            "Stop" => {
                let reason = payload
                    .reason
                    .unwrap_or_else(|| "response ready".to_string());
                self.status_text = reason.clone();
                self.push_activity(ActivityLevel::Info, "turn finished", reason);
            }
            "SubagentStop" => {
                let reason = payload
                    .reason
                    .unwrap_or_else(|| "subagent finished".to_string());
                self.push_activity(ActivityLevel::Info, "subagent stop", reason);
            }
            _ => {
                self.push_activity(ActivityLevel::Info, event_name, "hook event received");
            }
        }
    }

    fn handle_response(&mut self, response: SessionInvokeResponse) {
        self.busy = false;
        self.todos = response.todos;
        self.status_text = "assistant reply ready".to_string();

        if let Some(message) = response.latest_assistant_message {
            self.push_message(map_role(message.role), message.content);
        }

        self.fold_hook_summary(&response.hook_output);

        if let Some(snapshot) = &response.workspace_snapshot {
            self.push_activity(
                ActivityLevel::Info,
                "workspace snapshot",
                format!("{} files included in response", snapshot.len()),
            );
        }
    }

    fn fold_hook_summary(&mut self, summary: &HookOutputSummary) {
        for message in &summary.messages {
            self.push_activity(ActivityLevel::Info, "hook note", message.clone());
        }
        for tool in &summary.suppressed_tools {
            self.push_activity(
                ActivityLevel::Warn,
                "tool output suppressed",
                format!("{tool} was redacted by a hook"),
            );
        }
        for output in &summary.outputs {
            self.push_activity(
                ActivityLevel::Info,
                "hook output",
                summarize_value(output, 140),
            );
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum MessageRole {
    User,
    Assistant,
    System,
}

#[derive(Debug)]
struct TranscriptEntry {
    role: MessageRole,
    body: String,
}

impl TranscriptEntry {
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
}

#[derive(Debug, Clone, Copy)]
enum ActivityLevel {
    Info,
    Success,
    Warn,
    Error,
}

#[derive(Debug)]
struct ActivityEntry {
    elapsed: String,
    level: ActivityLevel,
    title: String,
    detail: String,
}

impl ActivityEntry {
    fn as_list_item(&self) -> ListItem<'static> {
        let title_color = match self.level {
            ActivityLevel::Info => NEON_CYAN,
            ActivityLevel::Success => NEON_GREEN,
            ActivityLevel::Warn => HOT_PINK,
            ActivityLevel::Error => HOT_PINK,
        };

        let title_style = Style::default()
            .fg(rgb_color(title_color))
            .add_modifier(Modifier::BOLD);

        ListItem::new(Text::from(vec![
            Line::from(vec![
                Span::styled(
                    self.elapsed.clone(),
                    Style::default().fg(rgb_color(DARK_GRAY)),
                ),
                Span::raw(" "),
                Span::styled(self.title.clone(), title_style),
            ]),
            Line::from(Span::styled(
                self.detail.clone(),
                Style::default().fg(rgb_color(MEDIUM_GRAY)),
            )),
        ]))
    }
}

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

fn panel<'a>(title: &'a str) -> Block<'a> {
    Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .title(Span::styled(
            format!(" {title} "),
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        ))
}

fn centered_rect(horizontal_percent: u16, vertical_percent: u16, area: Rect) -> Rect {
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - vertical_percent) / 2),
            Constraint::Percentage(vertical_percent),
            Constraint::Percentage((100 - vertical_percent) / 2),
        ])
        .split(area);
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - horizontal_percent) / 2),
            Constraint::Percentage(horizontal_percent),
            Constraint::Percentage((100 - horizontal_percent) / 2),
        ])
        .flex(Flex::Center)
        .split(vertical[1])[1]
}

fn trim_queue<T>(items: &mut VecDeque<T>, max_items: usize) {
    while items.len() > max_items {
        let _ = items.pop_front();
    }
}

fn summarize_value(value: &Value, max_len: usize) -> String {
    let rendered = match value {
        Value::String(text) => Cow::Borrowed(text.as_str()),
        _ => Cow::Owned(serde_json::to_string(value).unwrap_or_else(|_| value.to_string())),
    };
    truncate_text(rendered, max_len)
}

fn truncate_text(text: impl AsRef<str>, max_len: usize) -> String {
    let text = text.as_ref().trim();
    if text.len() <= max_len {
        return text.to_string();
    }
    let mut truncated = text
        .chars()
        .take(max_len.saturating_sub(3))
        .collect::<String>();
    truncated.push_str("...");
    truncated
}

fn short_call_id(tool_call_id: Option<&str>) -> String {
    tool_call_id
        .and_then(|value| value.rsplit('-').next())
        .map_or_else(String::new, |value| format!("#{value}"))
}

fn map_role(role: Role) -> MessageRole {
    match role {
        Role::User => MessageRole::User,
        Role::Assistant => MessageRole::Assistant,
        Role::Tool => MessageRole::System,
    }
}

fn format_elapsed(elapsed: Duration) -> String {
    let total_secs = elapsed.as_secs();
    let minutes = total_secs / 60;
    let seconds = total_secs % 60;
    format!("+{minutes:02}:{seconds:02}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn summarize_value_truncates_long_payloads() {
        let summary = summarize_value(&json!({"payload": "abcdefghijklmnopqrstuvwxyz"}), 18);
        assert!(summary.ends_with("..."));
        assert!(summary.len() <= 18);
    }

    #[test]
    fn short_call_id_prefers_last_dash_segment() {
        assert_eq!(short_call_id(Some("invoke-123-7")), "#7");
        assert_eq!(short_call_id(None), "");
    }
}
