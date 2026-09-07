use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::io::{self, Stdout};

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use miette::miette;
use minecraft_analysis_core::chunk_blocks::{BlockIndex, BlockRecord};
use minecraft_analysis_core::nbt::{Compression, Document, Value};
use minecraft_analysis_core::region::ChunkCompression;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Modifier, Style};
use ratatui::text::Line;
use ratatui::widgets::{Paragraph, Row, Table, Wrap};
use ratatui::{Frame, Terminal};

use crate::nbt_context::IdentityContext;
use crate::nbt_input::Source;

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum Segment {
    Key(String),
    Index(usize),
}

type NbtPath = Vec<Segment>;

#[derive(Clone, Copy)]
enum Node<'a> {
    Root(&'a std::collections::BTreeMap<String, Value>),
    Value(&'a Value),
}

impl Node<'_> {
    fn child_count(&self) -> Option<usize> {
        match self {
            Self::Root(values) => Some(values.len()),
            Self::Value(Value::Compound(values)) => Some(values.len()),
            Self::Value(Value::List(list)) => Some(list.values.len()),
            Self::Value(_) => None,
        }
    }
}

fn resolve<'a>(document: &'a Document, path: &[Segment]) -> Option<Node<'a>> {
    if path.is_empty() {
        return Some(Node::Root(&document.root));
    }
    let mut value = match &path[0] {
        Segment::Key(key) => document.root.get(key)?,
        Segment::Index(_) => return None,
    };
    for segment in &path[1..] {
        value = match (value, segment) {
            (Value::Compound(values), Segment::Key(key)) => values.get(key)?,
            (Value::List(list), Segment::Index(index)) => list.values.get(*index)?,
            _ => return None,
        };
    }
    Some(Node::Value(value))
}

fn first_child(document: &Document, path: &[Segment]) -> Option<NbtPath> {
    let segment = match resolve(document, path)? {
        Node::Root(values) | Node::Value(Value::Compound(values)) => {
            Segment::Key(values.keys().next()?.clone())
        }
        Node::Value(Value::List(list_values)) if !list_values.values.is_empty() => {
            Segment::Index(0)
        }
        Node::Value(_) => return None,
    };
    let mut child = path.to_vec();
    child.push(segment);
    Some(child)
}

fn next_sibling(document: &Document, path: &[Segment]) -> Option<NbtPath> {
    let (last, parent) = path.split_last()?;
    let next = match (resolve(document, parent)?, last) {
        (Node::Root(values) | Node::Value(Value::Compound(values)), Segment::Key(key)) => {
            Segment::Key(
                values
                    .keys()
                    .skip_while(|candidate| *candidate != key)
                    .nth(1)?
                    .clone(),
            )
        }
        (Node::Value(Value::List(list_values)), Segment::Index(index))
            if index + 1 < list_values.values.len() =>
        {
            Segment::Index(index + 1)
        }
        _ => return None,
    };
    let mut result = parent.to_vec();
    result.push(next);
    Some(result)
}

fn next_visible(
    document: &Document,
    expanded: &BTreeSet<NbtPath>,
    path: &[Segment],
) -> Option<NbtPath> {
    if expanded.contains(path) {
        if let Some(child) = first_child(document, path) {
            return Some(child);
        }
    }
    let mut cursor = path.to_vec();
    loop {
        if let Some(sibling) = next_sibling(document, &cursor) {
            return Some(sibling);
        }
        cursor.pop()?;
    }
}

fn previous_visible(
    document: &Document,
    expanded: &BTreeSet<NbtPath>,
    path: &[Segment],
) -> Option<NbtPath> {
    let (last, parent) = path.split_last()?;
    let previous = match (resolve(document, parent)?, last) {
        (Node::Root(values) | Node::Value(Value::Compound(values)), Segment::Key(key)) => values
            .keys()
            .take_while(|candidate| *candidate != key)
            .last()
            .map(|key| Segment::Key(key.clone())),
        (Node::Value(Value::List(_)), Segment::Index(index)) if *index > 0 => {
            Some(Segment::Index(index - 1))
        }
        _ => None,
    };
    let Some(previous) = previous else {
        return Some(parent.to_vec());
    };
    let mut cursor = parent.to_vec();
    cursor.push(previous);
    while expanded.contains(&cursor) {
        let Some(mut child) = last_child(document, &cursor) else {
            break;
        };
        std::mem::swap(&mut cursor, &mut child);
    }
    Some(cursor)
}

fn last_child(document: &Document, path: &[Segment]) -> Option<NbtPath> {
    let segment = match resolve(document, path)? {
        Node::Root(values) | Node::Value(Value::Compound(values)) => {
            Segment::Key(values.keys().next_back()?.clone())
        }
        Node::Value(Value::List(list_values)) if !list_values.values.is_empty() => {
            Segment::Index(list_values.values.len() - 1)
        }
        Node::Value(_) => return None,
    };
    let mut result = path.to_vec();
    result.push(segment);
    Some(result)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Action {
    Previous,
    Next,
    Left,
    Right,
    Enter,
    Exit,
    ToggleMode,
    ToggleAir,
    Jump,
}

fn action_for_key(key: KeyEvent) -> Option<Action> {
    if key.kind != KeyEventKind::Press {
        return None;
    }
    match key.code {
        KeyCode::Up | KeyCode::Char('k') => Some(Action::Previous),
        KeyCode::Down | KeyCode::Char('j') => Some(Action::Next),
        KeyCode::Left | KeyCode::Char('h') => Some(Action::Left),
        KeyCode::Right | KeyCode::Char('l') => Some(Action::Right),
        KeyCode::Enter => Some(Action::Enter),
        KeyCode::Esc | KeyCode::Char('q') => Some(Action::Exit),
        KeyCode::Tab => Some(Action::ToggleMode),
        KeyCode::Char('a') => Some(Action::ToggleAir),
        KeyCode::Char('g') => Some(Action::Jump),
        _ => None,
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Mode {
    Nbt,
    Blocks,
}

#[derive(Default)]
struct BlockState {
    selected: usize,
    viewport_top: usize,
    show_air: bool,
    reveal_filtered: Option<usize>,
    prompt: Option<String>,
    message: Option<String>,
}

struct App<'a> {
    document: &'a Document,
    selected: NbtPath,
    viewport_top: NbtPath,
    expanded: BTreeSet<NbtPath>,
    quit: bool,
    blocks: Option<&'a BlockIndex<'a>>,
    block_state: BlockState,
    mode: Mode,
    blocks_unavailable: Option<String>,
}

impl<'a> App<'a> {
    fn new(
        document: &'a Document,
        blocks: Option<&'a BlockIndex<'a>>,
        blocks_unavailable: Option<String>,
    ) -> Self {
        let mut app = Self {
            document,
            selected: vec![],
            viewport_top: vec![],
            expanded: BTreeSet::new(),
            quit: false,
            blocks,
            block_state: BlockState::default(),
            mode: if blocks.is_some() {
                Mode::Blocks
            } else {
                Mode::Nbt
            },
            blocks_unavailable,
        };
        app.normalize_block_selection();
        app
    }

    fn apply(&mut self, action: Action) {
        if self.mode == Mode::Blocks {
            self.apply_blocks(action);
            return;
        }
        match action {
            Action::Previous => {
                if let Some(path) = previous_visible(self.document, &self.expanded, &self.selected)
                {
                    self.selected = path;
                }
            }
            Action::Next => {
                if let Some(path) = next_visible(self.document, &self.expanded, &self.selected) {
                    self.selected = path;
                }
            }
            Action::Left => {
                if self.expanded.remove(&self.selected) {
                    return;
                }
                if !self.selected.is_empty() {
                    self.selected.pop();
                }
            }
            Action::Right => {
                if resolve(self.document, &self.selected)
                    .and_then(|node| node.child_count())
                    .is_some()
                {
                    if self.expanded.insert(self.selected.clone()) {
                        return;
                    }
                    if let Some(path) = first_child(self.document, &self.selected) {
                        self.selected = path;
                    }
                }
            }
            Action::Enter => {
                if resolve(self.document, &self.selected)
                    .and_then(|node| node.child_count())
                    .is_some()
                    && !self.expanded.insert(self.selected.clone())
                {
                    self.expanded.remove(&self.selected);
                }
            }
            Action::Exit => self.quit = true,
            Action::ToggleMode if self.blocks.is_some() => self.mode = Mode::Blocks,
            Action::ToggleMode | Action::ToggleAir | Action::Jump => {}
        }
    }

    fn apply_blocks(&mut self, action: Action) {
        match action {
            Action::Previous => self.move_block(-1),
            Action::Next => self.move_block(1),
            Action::ToggleAir => {
                self.block_state.show_air = !self.block_state.show_air;
                self.block_state.reveal_filtered = None;
                self.normalize_block_selection();
            }
            Action::Jump => {
                self.block_state.prompt = Some(String::new());
                self.block_state.message = None;
            }
            Action::ToggleMode => self.mode = Mode::Nbt,
            Action::Exit => self.quit = true,
            Action::Left | Action::Right | Action::Enter => {}
        }
    }

    fn visible_block_indices(&self) -> Vec<usize> {
        let Some(index) = self.blocks else {
            return vec![];
        };
        index
            .records()
            .iter()
            .enumerate()
            .filter_map(|(position, record)| {
                (self.block_state.show_air
                    || record.id != 0
                    || self.block_state.reveal_filtered == Some(position))
                .then_some(position)
            })
            .collect()
    }

    fn move_block(&mut self, delta: isize) {
        let visible = self.visible_block_indices();
        if visible.is_empty() {
            return;
        }
        let current = visible
            .iter()
            .position(|value| *value == self.block_state.selected)
            .unwrap_or(0);
        let next = current.saturating_add_signed(delta).min(visible.len() - 1);
        self.block_state.selected = visible[next];
        self.block_state.reveal_filtered = None;
    }

    fn normalize_block_selection(&mut self) {
        let visible = self.visible_block_indices();
        if !visible.contains(&self.block_state.selected) {
            self.block_state.selected = visible.first().copied().unwrap_or(0);
        }
    }

    fn handle_key(&mut self, key: KeyEvent) {
        if key.kind != KeyEventKind::Press {
            return;
        }
        if let Some(prompt) = self.block_state.prompt.as_mut() {
            match key.code {
                KeyCode::Esc => self.block_state.prompt = None,
                KeyCode::Backspace => {
                    prompt.pop();
                }
                KeyCode::Enter => self.submit_jump(),
                KeyCode::Char(character) => prompt.push(character),
                _ => {}
            }
            return;
        }
        if let Some(action) = action_for_key(key) {
            self.apply(action);
        }
    }

    fn submit_jump(&mut self) {
        let input = self.block_state.prompt.take().unwrap_or_default();
        let parsed = parse_block_coordinate(&input);
        let coordinate = match parsed {
            Ok(value) => value,
            Err(error) => {
                self.block_state.message = Some(error);
                return;
            }
        };
        let Some(index) = self.blocks else { return };
        let min_x = index.chunk[0].saturating_mul(16);
        let min_z = index.chunk[1].saturating_mul(16);
        if !(min_x..=min_x.saturating_add(15)).contains(&coordinate[0])
            || !(min_z..=min_z.saturating_add(15)).contains(&coordinate[2])
        {
            self.block_state.message = Some(format!(
                "coordinate is outside chunk x={min_x}..{}, z={min_z}..{}",
                min_x.saturating_add(15),
                min_z.saturating_add(15)
            ));
            return;
        }
        let Some(position) = index.position(coordinate) else {
            self.block_state.message = Some(format!(
                "no stored block is indexed at ({},{},{})",
                coordinate[0], coordinate[1], coordinate[2]
            ));
            return;
        };
        self.block_state.selected = position;
        self.block_state.reveal_filtered =
            (index.records()[position].id == 0 && !self.block_state.show_air).then_some(position);
        self.block_state.message = None;
    }

    fn visible_from(&self, start: &NbtPath, limit: usize) -> Vec<NbtPath> {
        let mut rows = vec![start.clone()];
        while rows.len() < limit {
            let Some(next) = next_visible(
                self.document,
                &self.expanded,
                rows.last().expect("row exists"),
            ) else {
                break;
            };
            rows.push(next);
        }
        rows
    }

    fn ensure_visible(&mut self, height: usize) {
        if height == 0 {
            return;
        }
        let rows = self.visible_from(&self.viewport_top, height);
        if rows.contains(&self.selected) {
            return;
        }
        if self.selected < self.viewport_top {
            self.viewport_top = self.selected.clone();
            return;
        }
        let mut top = self.selected.clone();
        for _ in 1..height {
            let Some(previous) = previous_visible(self.document, &self.expanded, &top) else {
                break;
            };
            top = previous;
        }
        self.viewport_top = top;
    }
}

fn parse_block_coordinate(value: &str) -> Result<[i32; 3], String> {
    let parts = value.split(',').collect::<Vec<_>>();
    if parts.len() != 3 || parts.iter().any(|part| part.is_empty()) {
        return Err("expected coordinates in x,y,z form".into());
    }
    let mut result = [0; 3];
    for (index, part) in parts.into_iter().enumerate() {
        result[index] = part
            .parse()
            .map_err(|_| format!("invalid coordinate {part:?}"))?;
    }
    Ok(result)
}

fn compression_name(compression: Compression) -> &'static str {
    match compression {
        Compression::Uncompressed => "uncompressed",
        Compression::Gzip => "gzip",
        Compression::Zlib => "zlib",
    }
}

fn chunk_compression_name(compression: ChunkCompression) -> &'static str {
    match compression {
        ChunkCompression::Gzip => "gzip",
        ChunkCompression::Zlib => "zlib",
        ChunkCompression::Uncompressed => "uncompressed",
    }
}

fn source_header(source: &Source, root_name: &str) -> String {
    match source {
        Source::Standalone { path, compression } => format!(
            "{} | {} | root {:?}",
            path.display(),
            compression_name(*compression),
            root_name
        ),
        Source::RegionChunk {
            path,
            compression,
            selection,
        } => format!(
            "{} | {} | root {:?} | global ({},{}) | local ({},{})",
            path.display(),
            chunk_compression_name(*compression),
            root_name,
            selection.global.x,
            selection.global.z,
            selection.local.x,
            selection.local.z
        ),
    }
}

fn label(path: &[Segment]) -> String {
    match path.last() {
        None => "<root>".into(),
        Some(Segment::Key(key)) => key.clone(),
        Some(Segment::Index(index)) => format!("[{index}]"),
    }
}

fn preview(node: Node<'_>) -> (String, String) {
    match node {
        Node::Root(values) => ("Compound".into(), format!("{} entries", values.len())),
        Node::Value(value) => match value {
            Value::Byte(v) => ("Byte".into(), format!("{v}b")),
            Value::Short(v) => ("Short".into(), format!("{v}s")),
            Value::Int(v) => ("Int".into(), v.to_string()),
            Value::Long(v) => ("Long".into(), format!("{v}L")),
            Value::Float(v) => ("Float".into(), format!("{v}f")),
            Value::Double(v) => ("Double".into(), format!("{v}d")),
            Value::String(v) => ("String".into(), format!("{v:?}")),
            Value::Compound(v) => ("Compound".into(), format!("{} entries", v.len())),
            Value::List(v) => (
                "List".into(),
                format!("{} elements ({:?})", v.values.len(), v.element_tag),
            ),
            Value::ByteArray(v) => ("ByteArray".into(), array_preview(v)),
            Value::IntArray(v) => ("IntArray".into(), array_preview(v)),
            Value::LongArray(v) => ("LongArray".into(), array_preview(v)),
        },
    }
}

fn array_preview<T: std::fmt::Display>(values: &[T]) -> String {
    let items = values
        .iter()
        .take(8)
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(", ");
    let suffix = if values.len() > 8 { ", …" } else { "" };
    format!("{} elements [{items}{suffix}]", values.len())
}

fn draw_nbt(
    frame: &mut Frame<'_>,
    app: &mut App<'_>,
    source: &Source,
    identity: Option<&IdentityContext>,
) {
    let areas = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(1),
        Constraint::Length(1),
    ])
    .split(frame.area());
    frame.render_widget(
        Paragraph::new(viewer_header(
            source,
            &app.document.root_name,
            identity,
            app,
        )),
        areas[0],
    );
    let height = usize::from(areas[1].height.saturating_sub(1));
    app.ensure_visible(height);
    let rows = app
        .visible_from(&app.viewport_top, height)
        .into_iter()
        .map(|path| {
            let node = resolve(app.document, &path).expect("application paths remain valid");
            let container = node.child_count().is_some();
            let disclosure = if container {
                if app.expanded.contains(&path) {
                    "▼"
                } else {
                    "▶"
                }
            } else {
                " "
            };
            let (kind, value) = preview(node);
            let name = format!("{}{} {}", "  ".repeat(path.len()), disclosure, label(&path));
            let style = if path == app.selected {
                Style::default().add_modifier(Modifier::REVERSED)
            } else {
                Style::default()
            };
            Row::new(vec![name, kind, value]).style(style)
        });
    let table = Table::new(
        rows,
        [
            Constraint::Percentage(45),
            Constraint::Length(12),
            Constraint::Min(1),
        ],
    )
    .header(Row::new(["Name", "Type", "Value"]));
    frame.render_widget(table, areas[1]);
    frame.render_widget(
        Paragraph::new(Line::from(if app.blocks.is_some() {
            "Tab Blocks  ↑/k ↓/j move  ←/h collapse/parent  →/l expand/child  Enter toggle  q/Esc exit"
        } else {
            "↑/k ↓/j move  ←/h collapse/parent  →/l expand/child  Enter toggle  q/Esc exit"
        })),
        areas[2],
    );
}

fn viewer_header(
    source: &Source,
    root_name: &str,
    identity: Option<&IdentityContext>,
    app: &App<'_>,
) -> String {
    let mut header = source_header(source, root_name);
    if let Some(identity) = identity {
        let assumed = if identity.assumed { " assumed" } else { "" };
        write!(header, " | {}{assumed}", identity.profile.label())
            .expect("writing to String cannot fail");
        write!(header, " | catalogs {}", identity.sources.join(", "))
            .expect("writing to String cannot fail");
        if !identity.warnings.is_empty() {
            header.push_str(" | ⚠ context warning");
        }
    }
    if let Some(reason) = &app.blocks_unavailable {
        write!(header, " | Blocks unavailable: {reason}").expect("writing to String cannot fail");
    }
    header
}

fn draw_blocks(
    frame: &mut Frame<'_>,
    app: &mut App<'_>,
    source: &Source,
    identity: Option<&IdentityContext>,
) {
    let areas = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(1),
        Constraint::Length(2),
    ])
    .split(frame.area());
    frame.render_widget(
        Paragraph::new(viewer_header(
            source,
            &app.document.root_name,
            identity,
            app,
        )),
        areas[0],
    );
    let body = Layout::horizontal([Constraint::Percentage(62), Constraint::Percentage(38)])
        .split(areas[1]);
    let visible = app.visible_block_indices();
    let height = usize::from(body[0].height.saturating_sub(1));
    let selected_visible = visible
        .iter()
        .position(|value| *value == app.block_state.selected)
        .unwrap_or(0);
    if selected_visible < app.block_state.viewport_top {
        app.block_state.viewport_top = selected_visible;
    } else if height > 0 && selected_visible >= app.block_state.viewport_top.saturating_add(height)
    {
        app.block_state.viewport_top = selected_visible + 1 - height;
    }
    let Some(index) = app.blocks else { return };
    let rows = visible
        .iter()
        .skip(app.block_state.viewport_top)
        .take(height)
        .map(|position| {
            let record = &index.records()[*position];
            let name = record.identity.as_ref().map_or_else(
                || format!("<unknown:{}>", record.id),
                |identity| identity.name.clone(),
            );
            let style = if *position == app.block_state.selected {
                Style::default().add_modifier(Modifier::REVERSED)
            } else {
                Style::default()
            };
            Row::new(vec![
                format!(
                    "{},{},{}",
                    record.coordinate[0], record.coordinate[1], record.coordinate[2]
                ),
                name,
                record.id.to_string(),
                record.metadata.to_string(),
            ])
            .style(style)
        });
    frame.render_widget(
        Table::new(
            rows,
            [
                Constraint::Length(18),
                Constraint::Min(20),
                Constraint::Length(6),
                Constraint::Length(5),
            ],
        )
        .header(Row::new(["x,y,z", "Block", "ID", "Data"])),
        body[0],
    );
    let detail = visible
        .contains(&app.block_state.selected)
        .then(|| index.records().get(app.block_state.selected))
        .flatten()
        .map_or_else(|| "No visible blocks".into(), block_detail);
    frame.render_widget(Paragraph::new(detail).wrap(Wrap { trim: false }), body[1]);
    let footer = if let Some(prompt) = &app.block_state.prompt {
        format!("Jump to x,y,z: {prompt}")
    } else {
        let air = if app.block_state.show_air {
            "hide air"
        } else {
            "show air"
        };
        let message = app
            .block_state
            .message
            .as_deref()
            .or_else(|| identity.and_then(|context| context.warnings.first().map(String::as_str)))
            .unwrap_or("");
        format!("g jump  a {air}  Tab NBT  ↑/k ↓/j move  q/Esc exit\n{message}")
    };
    frame.render_widget(Paragraph::new(footer), areas[2]);
}

fn block_detail(record: &BlockRecord<'_>) -> String {
    let identity = record.identity.as_ref().map_or_else(
        || format!("identity: <unknown:{}>", record.id),
        |identity| {
            format!(
                "identity: {}\nsource: {} ({})",
                identity.name, identity.provenance_source, identity.provenance_detail
            )
        },
    );
    let block_light = record
        .block_light
        .map_or_else(|| "—".into(), |value| value.to_string());
    let sky_light = record
        .sky_light
        .map_or_else(|| "—".into(), |value| value.to_string());
    let entity = record.block_entity.map_or_else(
        || "none".into(),
        |value| {
            minecraft_analysis_core::nbt::value_to_snbt(value)
                .unwrap_or_else(|error| format!("<cannot render: {error}>"))
        },
    );
    format!(
        "coordinate: {},{},{}\n{identity}\nid: {}\nmetadata: {}\nblock light: {block_light}\nsky light: {sky_light}\nsection Y: {}\nsection index: {}\nblock entity:\n{entity}",
        record.coordinate[0], record.coordinate[1], record.coordinate[2], record.id, record.metadata, record.section_y, record.section_index
    )
}

fn draw(
    frame: &mut Frame<'_>,
    app: &mut App<'_>,
    source: &Source,
    identity: Option<&IdentityContext>,
) {
    match app.mode {
        Mode::Nbt => draw_nbt(frame, app, source, identity),
        Mode::Blocks => draw_blocks(frame, app, source, identity),
    }
}

struct Session {
    terminal: Terminal<CrosstermBackend<Stdout>>,
    restored: bool,
}

impl Session {
    fn enter() -> io::Result<Self> {
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        if let Err(error) = execute!(stdout, EnterAlternateScreen) {
            let _ = disable_raw_mode();
            return Err(error);
        }
        match Terminal::new(CrosstermBackend::new(stdout)) {
            Ok(terminal) => Ok(Self {
                terminal,
                restored: false,
            }),
            Err(error) => {
                let mut stdout = io::stdout();
                let _ = execute!(stdout, LeaveAlternateScreen);
                let _ = disable_raw_mode();
                Err(error)
            }
        }
    }

    fn restore(&mut self) -> io::Result<()> {
        let screen = execute!(self.terminal.backend_mut(), LeaveAlternateScreen);
        let raw = disable_raw_mode();
        self.restored = true;
        screen.and(raw)
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        if !self.restored {
            let _ = self.restore();
        }
    }
}

pub fn run(
    source: &Source,
    document: &Document,
    identity: Option<&IdentityContext>,
) -> miette::Result<()> {
    let block_result = match (source, identity) {
        (Source::RegionChunk { selection, .. }, Some(identity)) => Some(BlockIndex::build(
            document,
            [selection.global.x, selection.global.z],
            &identity.catalog,
        )),
        _ => None,
    };
    let (block_index, unavailable) = match block_result.as_ref() {
        Some(Ok(index)) => (Some(index), None),
        Some(Err(error)) => (None, Some(error.to_string())),
        None => (None, None),
    };
    let mut session = Session::enter()
        .map_err(|error| miette!("cannot initialize NBT viewer terminal: {error}"))?;
    let mut app = App::new(document, block_index, unavailable);
    let application = (|| -> io::Result<()> {
        while !app.quit {
            session
                .terminal
                .draw(|frame| draw(frame, &mut app, source, identity))?;
            if let Event::Key(key) = event::read()? {
                app.handle_key(key);
            }
        }
        Ok(())
    })();
    let cleanup = session.restore();
    application
        .map_err(|error| miette!("NBT viewer rendering or event handling failed: {error}"))?;
    cleanup.map_err(|error| miette!("cannot restore terminal after NBT viewer: {error}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::path::Path;

    use crossterm::event::KeyModifiers;
    use minecraft_analysis_core::nbt::{List, Tag};
    use minecraft_analysis_core::registry::{RegistryCatalog, VanillaVersion};
    use ratatui::backend::TestBackend;

    use super::*;

    fn document() -> Document {
        Document {
            root_name: "named-root".into(),
            root: BTreeMap::from([
                ("empty".into(), Value::Compound(BTreeMap::new())),
                ("number".into(), Value::Int(42)),
                (
                    "nested".into(),
                    Value::Compound(BTreeMap::from([(
                        "items".into(),
                        Value::List(List {
                            element_tag: Tag::String,
                            values: vec![
                                Value::String("first".into()),
                                Value::String("second".into()),
                            ],
                        }),
                    )])),
                ),
            ]),
        }
    }

    fn block_document() -> Document {
        let mut blocks = vec![0; 4096];
        blocks[0] = 1;
        let section = Value::Compound(BTreeMap::from([
            ("Y".into(), Value::Byte(0)),
            ("Blocks".into(), Value::ByteArray(blocks)),
            ("Data".into(), Value::ByteArray(vec![0; 2048])),
            ("BlockLight".into(), Value::ByteArray(vec![7; 2048])),
        ]));
        Document {
            root_name: String::new(),
            root: BTreeMap::from([(
                "Level".into(),
                Value::Compound(BTreeMap::from([
                    (
                        "Sections".into(),
                        Value::List(List {
                            element_tag: Tag::Compound,
                            values: vec![section],
                        }),
                    ),
                    (
                        "TileEntities".into(),
                        Value::List(List {
                            element_tag: Tag::Compound,
                            values: vec![],
                        }),
                    ),
                ])),
            )]),
        }
    }

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    #[test]
    fn resolves_compound_keys_and_list_indices() {
        let document = document();
        let path = vec![
            Segment::Key("nested".into()),
            Segment::Key("items".into()),
            Segment::Index(1),
        ];
        assert!(
            matches!(resolve(&document, &path), Some(Node::Value(Value::String(value))) if value == "second")
        );
        assert!(resolve(&document, &[Segment::Index(0)]).is_none());
    }

    #[test]
    fn navigation_handles_empty_nested_and_boundaries() {
        let document = document();
        let mut app = App::new(&document, None, None);
        app.apply(Action::Previous);
        assert!(app.selected.is_empty());
        app.apply(Action::Right);
        app.apply(Action::Right);
        assert_eq!(app.selected, vec![Segment::Key("empty".into())]);
        app.apply(Action::Right);
        assert!(app.expanded.contains(&app.selected));
        app.apply(Action::Next);
        assert_eq!(app.selected, vec![Segment::Key("nested".into())]);
        app.apply(Action::Right);
        app.apply(Action::Right);
        assert_eq!(app.selected.len(), 2);
        app.apply(Action::Right);
        app.apply(Action::Right);
        assert_eq!(app.selected.last(), Some(&Segment::Index(0)));
        app.apply(Action::Left);
        assert_eq!(app.selected.last(), Some(&Segment::Key("items".into())));
    }

    #[test]
    fn collapse_keeps_selection_and_then_moves_to_parent() {
        let document = document();
        let mut app = App::new(&document, None, None);
        app.apply(Action::Right);
        app.apply(Action::Right);
        app.apply(Action::Next);
        app.apply(Action::Right);
        assert!(app.expanded.contains(&app.selected));
        let selected = app.selected.clone();
        app.apply(Action::Left);
        assert_eq!(app.selected, selected);
        assert!(!app.expanded.contains(&app.selected));
        app.apply(Action::Left);
        assert!(app.selected.is_empty());
    }

    #[test]
    fn viewport_scrolls_to_keep_selection_visible() {
        let document = document();
        let mut app = App::new(&document, None, None);
        app.apply(Action::Right);
        app.apply(Action::Right);
        app.apply(Action::Next);
        app.apply(Action::Next);
        app.ensure_visible(2);
        assert_ne!(app.viewport_top, Vec::<Segment>::new());
        assert!(app
            .visible_from(&app.viewport_top, 2)
            .contains(&app.selected));
    }

    #[test]
    fn arrows_and_hjkl_map_to_equivalent_actions() {
        for (arrow, vim, expected) in [
            (KeyCode::Up, KeyCode::Char('k'), Action::Previous),
            (KeyCode::Down, KeyCode::Char('j'), Action::Next),
            (KeyCode::Left, KeyCode::Char('h'), Action::Left),
            (KeyCode::Right, KeyCode::Char('l'), Action::Right),
        ] {
            assert_eq!(action_for_key(key(arrow)), Some(expected));
            assert_eq!(action_for_key(key(vim)), Some(expected));
        }
        assert_eq!(action_for_key(key(KeyCode::Enter)), Some(Action::Enter));
        assert_eq!(action_for_key(key(KeyCode::Esc)), Some(Action::Exit));
        assert_eq!(action_for_key(key(KeyCode::Char('q'))), Some(Action::Exit));
        assert_eq!(action_for_key(key(KeyCode::Char('x'))), None);
    }

    fn render(document: &Document, width: u16, height: u16, expand: bool) -> String {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = App::new(document, None, None);
        if expand {
            app.expanded.insert(vec![]);
        }
        terminal
            .draw(|frame| {
                draw(
                    frame,
                    &mut app,
                    &Source::Standalone {
                        path: Path::new("fixture.nbt").into(),
                        compression: Compression::Gzip,
                    },
                    None,
                );
            })
            .unwrap();
        let buffer = terminal.backend().buffer();
        (0..height)
            .map(|y| {
                (0..width)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn rendering_shows_metadata_nested_rows_selection_and_footer() {
        let output = render(&document(), 100, 10, true);
        assert!(output.contains("fixture.nbt | gzip | root \"named-root\""));
        assert!(output.contains("empty"));
        assert!(output.contains("Compound"));
        assert!(output.contains("↑/k ↓/j"));
    }

    #[test]
    fn region_header_shows_chunk_source_metadata() {
        let source = Source::RegionChunk {
            path: "r.-2.0.mca".into(),
            compression: ChunkCompression::Zlib,
            selection: crate::nbt_input::ChunkSelection {
                region: crate::nbt_input::Coordinates { x: -2, z: 0 },
                global: crate::nbt_input::Coordinates { x: -33, z: 7 },
                local: crate::nbt_input::Coordinates { x: 31, z: 7 },
            },
        };
        assert_eq!(
            source_header(&source, "chunk-root"),
            "r.-2.0.mca | zlib | root \"chunk-root\" | global (-33,7) | local (31,7)"
        );
    }

    #[test]
    fn row_previews_cover_all_value_categories_and_bound_arrays() {
        let values = vec![
            Value::Byte(-1),
            Value::Short(2),
            Value::Int(3),
            Value::Long(4),
            Value::Float(5.5),
            Value::Double(6.5),
            Value::String("text".into()),
            Value::Compound(BTreeMap::new()),
            Value::List(List {
                element_tag: Tag::Int,
                values: vec![],
            }),
            Value::ByteArray((0..12).collect()),
            Value::IntArray((0..12).collect()),
            Value::LongArray((0..12).collect()),
        ];
        let expected = [
            "Byte",
            "Short",
            "Int",
            "Long",
            "Float",
            "Double",
            "String",
            "Compound",
            "List",
            "ByteArray",
            "IntArray",
            "LongArray",
        ];
        for (value, expected) in values.iter().zip(expected) {
            let (kind, text) = preview(Node::Value(value));
            assert_eq!(kind, expected);
            assert!(!text.is_empty());
        }
        let (_, bounded) = preview(Node::Value(values.last().unwrap()));
        assert!(bounded.contains('…'));
        assert!(!bounded.contains("11"));
    }

    #[test]
    fn narrow_terminal_rendering_does_not_panic() {
        let output = render(&document(), 12, 4, true);
        assert_eq!(output.lines().count(), 4);
    }

    #[test]
    fn block_mode_filters_air_and_jump_temporarily_reveals_it() {
        let document = block_document();
        let mut catalog = RegistryCatalog::default();
        catalog.add_vanilla_fallbacks(VanillaVersion::Minecraft1_7_10);
        let index = BlockIndex::build(&document, [0, 0], &catalog).unwrap();
        let mut app = App::new(&document, Some(&index), None);
        assert_eq!(app.mode, Mode::Blocks);
        assert_eq!(app.visible_block_indices(), vec![0]);
        app.apply(Action::Jump);
        for character in "1,0,0".chars() {
            app.handle_key(key(KeyCode::Char(character)));
        }
        app.handle_key(key(KeyCode::Enter));
        assert_eq!(app.block_state.selected, 1);
        assert_eq!(app.block_state.reveal_filtered, Some(1));
        assert_eq!(app.visible_block_indices(), vec![0, 1]);
        app.apply(Action::ToggleAir);
        assert_eq!(app.visible_block_indices().len(), 4096);
    }

    #[test]
    fn coordinate_prompt_reports_invalid_outside_and_absent_section_inputs() {
        let document = block_document();
        let index = BlockIndex::build(&document, [0, 0], &RegistryCatalog::default()).unwrap();
        let mut app = App::new(&document, Some(&index), None);
        for input in ["bad", "16,0,0", "0,32,0", "2147483648,0,0"] {
            app.block_state.prompt = Some(input.into());
            app.submit_jump();
            assert!(app.block_state.message.is_some());
            assert_eq!(app.block_state.selected, 0);
        }
        assert_eq!(
            parse_block_coordinate("-1,-16,-32").unwrap(),
            [-1, -16, -32]
        );
    }

    #[test]
    fn coordinate_prompt_supports_editing_and_prompt_scoped_escape() {
        let document = block_document();
        let index = BlockIndex::build(&document, [0, 0], &RegistryCatalog::default()).unwrap();
        let mut app = App::new(&document, Some(&index), None);
        app.handle_key(key(KeyCode::Char('g')));
        for character in "1,0,01".chars() {
            app.handle_key(key(KeyCode::Char(character)));
        }
        app.handle_key(key(KeyCode::Backspace));
        assert_eq!(app.block_state.prompt.as_deref(), Some("1,0,0"));
        app.handle_key(key(KeyCode::Esc));
        assert!(app.block_state.prompt.is_none());
        assert!(!app.quit);
        app.handle_key(key(KeyCode::Esc));
        assert!(app.quit);
    }

    #[test]
    fn block_mode_and_raw_mode_keep_independent_selection() {
        let document = block_document();
        let index = BlockIndex::build(&document, [0, 0], &RegistryCatalog::default()).unwrap();
        let mut app = App::new(&document, Some(&index), None);
        app.apply(Action::ToggleMode);
        assert_eq!(app.mode, Mode::Nbt);
        app.apply(Action::Right);
        app.apply(Action::Right);
        let raw_selection = app.selected.clone();
        app.apply(Action::ToggleMode);
        assert_eq!(app.mode, Mode::Blocks);
        app.apply(Action::ToggleMode);
        assert_eq!(app.selected, raw_selection);
    }

    #[test]
    fn block_render_shows_coordinates_identity_details_controls_and_warning() {
        let document = block_document();
        let mut catalog = RegistryCatalog::default();
        catalog.add_vanilla_fallbacks(VanillaVersion::Minecraft1_7_10);
        let identity = IdentityContext {
            catalog,
            profile: crate::nbt_context::ViewProfile::Forge1_7_10,
            assumed: true,
            sources: vec!["vanilla forge-1.7.10".into()],
            warnings: vec!["no ancestor level.dat found".into()],
        };
        let index = BlockIndex::build(&document, [0, 0], &identity.catalog).unwrap();
        for (width, height) in [(120, 16), (24, 5)] {
            let backend = TestBackend::new(width, height);
            let mut terminal = Terminal::new(backend).unwrap();
            let mut app = App::new(&document, Some(&index), None);
            terminal
                .draw(|frame| {
                    draw(
                        frame,
                        &mut app,
                        &Source::RegionChunk {
                            path: "r.0.0.mca".into(),
                            compression: ChunkCompression::Zlib,
                            selection: crate::nbt_input::ChunkSelection {
                                region: crate::nbt_input::Coordinates { x: 0, z: 0 },
                                global: crate::nbt_input::Coordinates { x: 0, z: 0 },
                                local: crate::nbt_input::Coordinates { x: 0, z: 0 },
                            },
                        },
                        Some(&identity),
                    );
                })
                .unwrap();
            let output = (0..height)
                .map(|y| {
                    (0..width)
                        .map(|x| terminal.backend().buffer()[(x, y)].symbol())
                        .collect::<String>()
                })
                .collect::<Vec<_>>()
                .join("\n");
            assert_eq!(output.lines().count(), usize::from(height));
            if width > 100 {
                assert!(output.contains("minecraft:stone"));
                assert!(output.contains("0,0,0"));
                assert!(output.contains("g jump"));
                assert!(output.contains("assumed"));
            }
        }
    }
}
