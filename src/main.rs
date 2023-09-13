//!

mod context_finder;
mod error;

use aho_corasick::AhoCorasick;
use context_finder::{ContextFinder, InputType};
use crossterm::{
    event::{read, DisableMouseCapture, EnableMouseCapture, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use error::Error;
use ratatui::{
    backend::{Backend, CrosstermBackend},
    layout::{Constraint, Direction, Layout},
    widgets::{Block, BorderType, Borders, Paragraph},
    Frame, Terminal,
};
use std::{
    io::{self, stdin, BufRead},
    sync::mpsc::{channel, Receiver, TryRecvError},
    thread::{self, JoinHandle},
    time::Duration,
};
use tracing::{error, trace, warn, Level};
use tui_input::{backend::crossterm::EventHandler, Input};

const INPUT_STREAM_TIMEOUT: u64 = 1000;
const ENVIRONMENT_VARIABLE_ENABLE_TRACING: &str = "ENABLE_TRACING";

fn main() -> Result<(), Error> {
    if let Ok(enable_tracing) = std::env::var(ENVIRONMENT_VARIABLE_ENABLE_TRACING) {
        if enable_tracing == "1" || &enable_tracing.to_lowercase() == "true" {
            let file_appender = tracing_appender::rolling::hourly("./.logs/", "runlog");
            tracing_subscriber::fmt()
                .with_max_level(Level::TRACE)
                .with_writer(file_appender)
                .init();
        }
    }
    trace!("Enabling raw mode");
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let res = run_app(&mut terminal);

    trace!("Disabling raw mode");

    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    if let Err(err) = res {
        error!("{:?}", err);
        eprintln!("{err}");
    }

    Ok(())
}

fn decrement(scroll: usize, count: usize) -> usize {
    if let Some(pos) = scroll.checked_sub(count) {
        pos
    } else {
        0
    }
}

fn increment(scroll: usize, count: usize, max_val: usize, vertical_size: u16) -> usize {
    if let Some(pos) = scroll.checked_add(count) {
        if pos > (max_val - vertical_size as usize) {
            max_val - vertical_size as usize
        } else {
            pos
        }
    } else {
        usize::MAX
    }
}

fn stream_input(num_lines: usize) -> (Receiver<Result<Vec<String>, Error>>, JoinHandle<()>) {
    trace!("Opening channel for input reader");
    let (tx, rx) = channel::<Result<Vec<String>, Error>>();
    let thread_handle = thread::spawn(move || {
        trace!("Reading input");
        let input = stdin().lock();
        trace!("Splitting input");
        let mut input_lines = input.split(b'\n');

        loop {
            trace!("Reading lines");
            let mut maybe_err = None;
            let mut lines = Vec::with_capacity(num_lines);
            for _ in 0..num_lines {
                match input_lines.next() {
                    Some(Ok(buf)) => {
                        trace!("Got lines");
                        let line = String::from_utf8_lossy(&buf).to_string();
                        lines.push(line);
                    }
                    Some(Err(err)) => {
                        warn!("Error reading input lines: {err}");
                        maybe_err = Some(err);
                        break;
                    }
                    None => {
                        trace!("No new lines");
                        return;
                    }
                }
            }
            if let Err(err) = tx.send(Ok(lines)) {
                warn!("Error sending input streaming result: {err}");
                return;
            }
            if let Some(read_err) = maybe_err {
                warn!("Got read error streaming input: {read_err}");
                if let Err(_send_err) = tx.send(Err(Error::StreamingSend)) {
                    return;
                }
            };
        }
    });
    (rx, thread_handle)
}

fn get_lines(log_lines: &[String], position: usize, vertical_size: u16) -> &[String] {
    trace!("Getting screenful of lines");
    let lines = if log_lines.len() > (position + vertical_size as usize) {
        log_lines.get(position..(position + vertical_size as usize))
    } else {
        log_lines.get(position..(log_lines.len() - 1))
    };
    lines.unwrap()
}

enum State {
    Pager,
    Search { term: Input },
}

fn run_app<B: Backend>(terminal: &mut Terminal<B>) -> Result<(), Error> {
    let mut position: usize = 0;
    let mut vertical_size = terminal.size()?.height;
    let (rx, _thread_handle) = stream_input((vertical_size as usize) * 4);
    let mut all_lines = rx.recv_timeout(Duration::from_millis(INPUT_STREAM_TIMEOUT))??;
    let cf = ContextFinder::new(InputType::Git)?;
    let mut state = State::Pager;

    loop {
        if let State::Search { ref term } = state {
            let ac = AhoCorasick::builder()
                .ascii_case_insensitive(true)
                .build([term.value()])?;
            let match_lines: Vec<usize> = all_lines
                .iter()
                .enumerate()
                .filter_map(|(line_num, line)| {
                    if ac.find_iter(line).next().is_some() {
                        Some(line_num)
                    } else {
                        None
                    }
                })
                .collect();
            position = *match_lines.first().unwrap_or(&position);
        }

        all_lines = match rx.try_recv() {
            Ok(maybe_new_lines) => {
                trace!("Got more lines");
                all_lines.extend(maybe_new_lines?);
                all_lines
            }
            Err(TryRecvError::Disconnected) => all_lines,
            Err(e) => {
                warn!("Got error receiving new lines: {e}");
                all_lines
            }
        };
        let context = cf.get_context(&all_lines[..], position);
        let lines = get_lines(&all_lines[..], position, terminal.size()?.height);

        terminal.draw(|frame| pager(frame, &state, lines, context, &mut vertical_size))?;

        let event = read()?;
        if let Event::Key(key) = event {
            match state {
                State::Pager => match key.code {
                    KeyCode::Char('q') => return Ok(()),
                    KeyCode::Char('j') | KeyCode::Down => {
                        position = increment(position, 1, all_lines.len(), vertical_size)
                    }
                    KeyCode::Char('k') | KeyCode::Up => position = decrement(position, 1),
                    KeyCode::PageDown => {
                        position = increment(
                            position,
                            vertical_size as usize,
                            all_lines.len(),
                            vertical_size,
                        )
                    }
                    KeyCode::PageUp => position = decrement(position, vertical_size as usize),
                    KeyCode::Char('/') => state = State::Search { term: "".into() },
                    _ => (),
                },
                State::Search { ref mut term } => match key.code {
                    KeyCode::Esc | KeyCode::Enter => state = State::Pager,
                    _ => {
                        term.handle_event(&event);
                    }
                },
            }
        }
    }
}

fn pager<B: Backend>(
    f: &mut Frame<B>,
    state: &State,
    git_log: &[String],
    commit: Option<&[String]>,
    vertical_size: &mut u16,
) {
    trace!("Rendering screen");
    let commit_len = commit.map(|commit| commit.iter().len() + 1).unwrap_or(0);
    let commit = commit.map(|commit| commit.join("\n"));

    let layout = match state {
        State::Search { .. } => vec![
            Constraint::Max(std::cmp::min(7, commit_len as u16)),
            Constraint::Min(8),
            Constraint::Max(2),
        ],
        State::Pager => vec![
            Constraint::Max(std::cmp::min(7, commit_len as u16)),
            Constraint::Min(8),
        ],
    };

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(layout.as_ref())
        .margin(1)
        .split(f.size());

    let commit_paragraph = Paragraph::new(commit.unwrap_or("".to_string())).block(
        Block::default()
            .borders(Borders::BOTTOM)
            .border_type(BorderType::Double),
    );
    f.render_widget(commit_paragraph, chunks[0]);

    let paragraph = Paragraph::new(git_log.join("\n")); //.scroll((*scroll, 0));
    f.render_widget(paragraph, chunks[1]);
    *vertical_size = chunks[1].height;

    match state {
        State::Search { term } => {
            let search_box = Paragraph::new(term.value()).block(
                Block::default()
                    .borders(Borders::TOP)
                    .border_type(BorderType::Plain),
            );
            f.render_widget(search_box, chunks[2]);
        }
        State::Pager => (),
    }
}
