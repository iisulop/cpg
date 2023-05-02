//!

mod context_finder;
mod error;

use context_finder::{ContextFinder, InputType};
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use error::CpgError;
use ratatui::{
    backend::{Backend, CrosstermBackend},
    layout::{Constraint, Direction, Layout},
    widgets::{Block, BorderType, Borders, Paragraph},
    Frame, Terminal,
};
use std::{
    io::{self, stdin, BufRead},
    sync::mpsc::{channel, Receiver},
    thread::{self, JoinHandle},
};

use tracing::error;

fn main() -> Result<(), CpgError> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let res = run_app(&mut terminal);

    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    if let Err(err) = res {
        error!("{:?}", err)
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

fn stream_input(num_lines: usize) -> (Receiver<Result<Vec<String>, CpgError>>, JoinHandle<()>) {
    let (tx, rx) = channel::<Result<Vec<String>, CpgError>>();
    let thread_handle = thread::spawn(move || {
        let input = stdin().lock();
        let mut input_lines = input.split(b'\n');

        loop {
            let mut maybe_err = None;
            let mut lines = Vec::with_capacity(num_lines);
            for _ in 0..num_lines {
                match input_lines.next() {
                    Some(Ok(buf)) => {
                        let line = String::from_utf8_lossy(&buf).to_string();
                        lines.push(line);
                    }
                    Some(Err(err)) => {
                        maybe_err = Some(err);
                        break;
                    }
                    None => return,
                }
            }
            if let Err(_err) = tx.send(Ok(lines)) {
                return;
            }
            if let Some(_read_err) = maybe_err {
                if let Err(_send_err) = tx.send(Err(CpgError::StreamingSendError)) {
                    return;
                }
            };
        }
    });
    (rx, thread_handle)
}

fn get_lines(log_lines: &[String], position: usize, vertical_size: u16) -> &[String] {
    let lines = if log_lines.len() > (position + vertical_size as usize) {
        log_lines.get(position..(position + vertical_size as usize))
    } else {
        log_lines.get(position..(log_lines.len() - 1))
    };
    lines.unwrap()
}

fn run_app<B: Backend>(terminal: &mut Terminal<B>) -> Result<(), CpgError> {
    let mut position: usize = 0;
    let mut vertical_size = terminal.size()?.height;
    let (rx, _thread_handle) = stream_input((vertical_size as usize) * 4);
    let mut all_lines = rx.recv()??;
    let cf = ContextFinder::new(InputType::Git)?;

    loop {
        all_lines = match rx.try_recv() {
            Ok(maybe_new_lines) => {
                all_lines.extend(maybe_new_lines?.into_iter());
                all_lines
            }
            Err(_) => all_lines,
        };
        let context = cf.get_context(&all_lines[..], position);
        let lines = get_lines(&all_lines[..], position, terminal.size()?.height);

        terminal.draw(|frame| pager(frame, lines, context, &mut vertical_size))?;

        if let Event::Key(key) = event::read()? {
            match key.code {
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
                _ => (),
            }
        }
    }
}

fn pager<B: Backend>(
    f: &mut Frame<B>,
    git_log: &[String],
    commit: Option<&[String]>,
    vertical_size: &mut u16,
) {
    let commit_len = commit.map(|commit| commit.iter().len() + 1).unwrap_or(0);
    let commit = commit.map(|commit| commit.join("\n"));
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(
            [
                Constraint::Max(std::cmp::min(7, commit_len as u16)),
                Constraint::Min(8),
            ]
            .as_ref(),
        )
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
}
