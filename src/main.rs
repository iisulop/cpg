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
    layout::{Constraint, Direction, Layout, Rect},
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
    scroll.checked_sub(count).unwrap_or_default()
}

fn increment(scroll: usize, count: usize, max_val: usize, vertical_size: u16) -> usize {
    if let Some(pos) = scroll.checked_add(count) {
        if pos > (max_val - usize::from(vertical_size)) {
            max_val - usize::from(vertical_size)
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

fn get_lines(
    log_lines: &[String],
    position: usize,
    vertical_size: u16,
) -> Result<&[String], Error> {
    trace!("Getting screenful of lines");
    let lines = if log_lines.len() > (position + usize::from(vertical_size)) {
        log_lines.get(position..(position + usize::from(vertical_size)))
    } else {
        log_lines.get(position..(log_lines.len() - 1))
    };
    lines.ok_or(Error::GetLines)
}

enum SearchState {
    GetInput { term: Input },
    Searching { term: Input, position: usize },
}

enum State {
    Pager,
    Search(SearchState),
}

#[derive(Debug, Eq, PartialEq)]
enum SearchDirection {
    Backwards,
    Forward,
}

fn run_app<B: Backend>(terminal: &mut Terminal<B>) -> Result<(), Error> {
    let mut position: usize = 0;
    let mut vertical_size = terminal.size()?.height;
    let (rx, _thread_handle) = stream_input(usize::from(vertical_size) * 4);
    let mut all_lines = rx.recv_timeout(Duration::from_millis(INPUT_STREAM_TIMEOUT))??;
    let cf = ContextFinder::new(&InputType::Git)?;
    let mut state = State::Pager;

    loop {
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
        let lines = get_lines(&all_lines[..], position, terminal.size()?.height)?;

        terminal.draw(|frame| pager(frame, &state, lines, context, &mut vertical_size))?;

        let event = read()?;
        if let Event::Key(key) = event {
            match state {
                State::Pager => match key.code {
                    KeyCode::Char('q') | KeyCode::Esc => return Ok(()),
                    KeyCode::Char('j') | KeyCode::Down => {
                        position = increment(position, 1, all_lines.len(), vertical_size);
                    }
                    KeyCode::Char('k') | KeyCode::Up => position = decrement(position, 1),
                    KeyCode::PageDown => {
                        position = increment(
                            position,
                            usize::from(vertical_size),
                            all_lines.len(),
                            vertical_size,
                        );
                    }
                    KeyCode::PageUp => position = decrement(position, usize::from(vertical_size)),
                    KeyCode::Char('/') => {
                        state = State::Search(SearchState::GetInput { term: "".into() });
                    }
                    _ => (),
                },
                State::Search(SearchState::GetInput { ref mut term }) => match key.code {
                    KeyCode::Esc => state = State::Pager,
                    KeyCode::Enter => {
                        state = State::Search(SearchState::Searching {
                            term: term.clone(),
                            position,
                        });
                    }
                    _ => {
                        position = search(term, position, &all_lines, &SearchDirection::Forward)?;
                        term.handle_event(&event);
                    }
                },
                State::Search(SearchState::Searching {
                    ref mut term,
                    position: _position,
                }) => match key.code {
                    KeyCode::Esc | KeyCode::Char('q') => state = State::Pager,
                    KeyCode::Char('n') => {
                        position =
                            search(term, position + 1, &all_lines, &SearchDirection::Forward)?;
                    }
                    KeyCode::Char('N') => {
                        position = search(term, position, &all_lines, &SearchDirection::Backwards)?;
                    }
                    _ => (),
                },
            }
        }
    }
}

fn search(
    term: &Input,
    position: usize,
    all_lines: &[String],
    direction: &SearchDirection,
) -> Result<usize, Error> {
    let ac = AhoCorasick::builder()
        .ascii_case_insensitive(true)
        .build([term.value()])?;
    let match_lines: Vec<usize> = match direction {
        SearchDirection::Backwards => all_lines
            .iter()
            .enumerate()
            .rev()
            .skip(all_lines.len() - position)
            .filter_map(|(line_num, line)| {
                if ac.find_iter(line).next().is_some() {
                    Some(line_num)
                } else {
                    None
                }
            })
            .collect(),
        SearchDirection::Forward => all_lines
            .iter()
            .enumerate()
            .skip(position)
            .filter_map(|(line_num, line)| {
                if ac.find_iter(line).next().is_some() {
                    Some(line_num)
                } else {
                    None
                }
            })
            .collect(),
    };
    Ok(*match_lines.first().unwrap_or(&position))
}

fn pager<B: Backend>(
    f: &mut Frame<B>,
    state: &State,
    git_log: &[String],
    commit: Option<&[String]>,
    vertical_size: &mut u16,
) {
    trace!("Rendering screen");
    let commit_len = commit.map_or(0, |commit| commit.iter().len() + 1);
    let commit = commit.map(|commit| commit.join("\n"));

    let layout = match state {
        State::Search { .. } => vec![
            #[allow(clippy::cast_possible_truncation)]
            Constraint::Max(std::cmp::min(7, commit_len as u16)),
            Constraint::Min(8),
            Constraint::Max(3),
        ],
        State::Pager => vec![
            #[allow(clippy::cast_possible_truncation)]
            Constraint::Max(std::cmp::min(7, commit_len as u16)),
            Constraint::Min(8),
        ],
    };

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(layout)
        .margin(1)
        .split(f.size());

    let commit_paragraph = Paragraph::new(commit.unwrap_or_default()).block(
        Block::default()
            .borders(Borders::BOTTOM)
            .border_type(BorderType::Double),
    );
    f.render_widget(commit_paragraph, chunks[0]);

    let paragraph = Paragraph::new(git_log.join("\n")); //.scroll((*scroll, 0));
    f.render_widget(paragraph, chunks[1]);
    *vertical_size = chunks[1].height;

    match state {
        State::Search(SearchState::GetInput { term }) => {
            draw_search_box(f, chunks[2], term);
        }
        State::Search(SearchState::Searching {
            term,
            position: _position,
        }) => {
            draw_search_box(f, chunks[2], term);
        }
        State::Pager => (),
    }
}

fn draw_search_box<B: Backend>(f: &mut Frame<B>, area: Rect, input: &Input) {
    // let search_box = Paragraph::new(input.value())
    // .block(Block::default().borders(Borders::ALL).title("Search"));
    // f.render_widget(search_box, area);
    let search_box =
        Paragraph::new(input.value()).block(Block::default().borders(Borders::ALL).title("Search"));
    f.render_widget(search_box, area);
}
