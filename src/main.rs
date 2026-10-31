#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod appearance;
mod assets;
mod emu;
mod gamepad;

use emu::{Config, Game, System};
use gamepad::Pad;
use gpui::{prelude::*, *};
use gpui_component::{
    Disableable, Root, Selectable, Theme, ThemeMode, TitleBar,
    button::{Button, ButtonVariants},
    input::{Input, InputEvent, InputState},
};

const MUTED: u32 = 0xa7b4ba;
const TEXT: u32 = 0xeeeae2;
const FONT: &str = if cfg!(target_os = "windows") { "Segoe UI" } else { ".SystemUIFont" };

#[derive(Clone, Copy, PartialEq)]
enum View {
    Home,
    Library,
    Settings,
}

/// Where the controller is, RetroArch style: the side menu, the game grid, or a Settings button.
#[derive(Clone, Copy, PartialEq)]
enum Focus {
    Menu(usize),
    Grid,
    Settings(usize),
    /// Home page: (row, column). Row 0 is Continue Playing, row 1 is Systems.
    Home(usize, usize),
}

/// A button shown in the bottom bar's hints.
#[derive(Clone, Copy)]
enum Hint {
    Confirm,
    Back,
    Options,
    Bumpers,
}

/// One side-menu entry: Home, a library filter (None = all games), or Settings.
#[derive(Clone, PartialEq)]
enum MenuItem {
    Home,
    Library(Option<String>),
    Settings,
}

/// The Settings buttons a controller can reach, top to bottom.
#[derive(Clone, Copy, PartialEq)]
enum Setting {
    /// Choose where the Emulation folder lives.
    EmulationFolder,
    RomFolders,
    RetroArch,
    Cores,
    Glass(bool),
    Palette(usize),
    /// Pick the program for emu::EMULATORS[i].
    PickEmulator(usize),
    /// Open emu::EMULATORS[i]'s official site.
    EmulatorSite(usize),
    /// Create and open Emulation/tools/<emulator>, to unzip it into.
    EmulatorFolder(usize),
}

/// The full-screen game page (right-click or Options). `focus` walks Play, Change cover, Back, then the cover choices.
struct Details {
    game: usize,
    /// None until "Change cover" is pressed; then the downloaded choices.
    candidates: Option<Vec<emu::Candidate>>,
    loading: bool,
    focus: usize,
    /// The cover choices scroll on their own, following the controller.
    scroll: ScrollHandle,
}

const DETAIL_BUTTONS: usize = 3;
/// Cover choices per row: the 740px choices box fits six 110px tiles with 16px gaps.
const CHOICE_COLUMNS: usize = 6;

const PALETTES: [(&str, u32, u32); 5] = [
    ("Teal", 0x0c1417, 0x92c8e5),
    ("Graphite", 0x151515, 0xc4c4ce),
    ("Blue", 0x101a2d, 0x92b9ff),
    ("Plum", 0x211324, 0xd5a8e3),
    ("Forest", 0x101f19, 0x91cba6),
];

/// The Settings buttons a controller can reach, in on-screen order: Setup, Emulators, Appearance.
fn settings_items() -> Vec<Setting> {
    let mut items = vec![Setting::EmulationFolder, Setting::RomFolders, Setting::RetroArch, Setting::Cores];
    for i in 0..emu::EMULATORS.len() {
        items.extend([Setting::EmulatorFolder(i), Setting::PickEmulator(i), Setting::EmulatorSite(i)]);
    }
    items.extend([Setting::Glass(false), Setting::Glass(true)]);
    items.extend((0..PALETTES.len()).map(Setting::Palette));
    items
}

struct Hub {
    query: Entity<InputState>,
    roms_dir: Entity<InputState>,
    retroarch_cmd: Entity<InputState>,
    cores_dir: Entity<InputState>,
    steamgriddb_key: Entity<InputState>,
    config: Config,
    systems: Vec<System>,
    games: Vec<Game>,
    view: View,
    system: Option<String>,
    selected: Option<usize>,
    sidebar_open: bool,
    busy: bool,
    focus: Focus,
    details: Option<Details>,
    /// Focus rings only show once a controller has been used, so mouse users don't see them.
    pad_used: bool,
    /// The emulator EmuHub started, while it's open: only one game at a time, and Share+Options closes it.
    running: Option<emu::Running>,
    /// Controller input is ignored while another window (e.g. a running game) has focus.
    active: bool,
    /// Cards per grid row, measured each frame for up/down navigation.
    columns: usize,
    /// The game grid only draws the rows on screen (uniform_list), which keeps big libraries smooth.
    grid_scroll: UniformListScrollHandle,
    /// Games shown in the grid this frame, in order (filter + search applied).
    visible_cache: Vec<usize>,
    /// Each game's cover file if it has one, parallel to `games`, so drawing never touches the disk.
    covers: Vec<Option<std::path::PathBuf>>,
    /// Bumped on every rescan so an older cover download pass stops.
    cover_pass: usize,
    /// Horizontal scroll of the Home rows, so the controller's card stays in view.
    home_scroll: [ScrollHandle; 2],
    /// Vertical scroll of the Home and Settings pages, so the controller's section stays in view.
    home_page_scroll: ScrollHandle,
    settings_scroll: ScrollHandle,
    /// When each game was last played, for Continue Playing.
    history: std::collections::HashMap<std::path::PathBuf, u64>,
    message: String,
    appearance: appearance::Appearance,
}

// The look below is shared with Project Launcher: icon(), tile(), heading(), card(), setting_row().
fn icon(name: &str, size: f32, color: impl Into<Hsla>) -> Svg {
    svg()
        .path(format!("icons/{name}.svg"))
        .w(px(size))
        .h(px(size))
        .flex_none()
        .text_color(color)
}

/// A rounded square with the first letter, colored from the name.
fn tile(name: &str, size: f32) -> Div {
    let hue = (name.bytes().fold(7u32, |h, b| h.wrapping_mul(31).wrapping_add(b as u32)) % 360) as f32 / 360.;
    div()
        .w(px(size))
        .h(px(size))
        .flex_none()
        .rounded_md()
        .flex()
        .items_center()
        .justify_center()
        .bg(hsla(hue, 0.38, 0.28, 1.))
        .text_color(hsla(hue, 0.7, 0.85, 1.))
        .font_weight(FontWeight::SEMIBOLD)
        .text_size(px(size * 0.42))
        .child(name.chars().next().map(|c| c.to_uppercase().to_string()).unwrap_or_default())
}

fn heading(text: &str) -> Div {
    div()
        .text_xs()
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(rgb(MUTED))
        .child(text.to_uppercase())
}

fn card() -> Div {
    div()
        .flex()
        .flex_col()
        .flex_none()
        .rounded_lg()
        .overflow_hidden()
        .border_1()
        .border_color(rgba(0xffffff14))
        .bg(rgba(0xffffff06))
}

/// One settings line: title and note on the left, add the control as a child.
fn setting_row(title: &str, note: &str) -> Div {
    let mut text = div()
        .flex_1()
        .min_w_0()
        .flex()
        .flex_col()
        .gap_1()
        .child(div().text_size(px(15.)).child(title.to_owned()));
    if !note.is_empty() {
        text = text.child(div().text_xs().text_color(rgb(MUTED)).child(note.to_owned()));
    }
    div()
        .flex()
        .items_center()
        .justify_between()
        .gap_6()
        .px_5()
        .py_3()
        .border_b_1()
        .border_color(rgba(0xffffff0f))
        .child(text)
}

fn system_name<'a>(systems: &'a [System], id: &'a str) -> &'a str {
    systems.iter().find(|s| s.id == id).map_or(id, |s| s.name.as_str())
}

impl Hub {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let appearance = appearance::Appearance::load();
        appearance.apply(window);
        appearance.apply_theme(cx);
        let config = Config::load();
        let mut field = |value: String| cx.new(|cx| InputState::new(window, cx).default_value(value));
        let roms_dir = field(config.roms_dir.display().to_string());
        // shortcut: splits on spaces, so a RetroArch path containing spaces won't work; add a proper args field if needed.
        let retroarch_cmd = field(config.retroarch_cmd.join(" "));
        let cores_dir = field(config.cores_dir.display().to_string());
        // Hidden like a password, with an eye button to show it while typing.
        let steamgriddb_key = cx.new(|cx| InputState::new(window, cx).masked(true).default_value(config.steamgriddb_key.clone()));
        // Saved as it's typed or pasted, so it doesn't depend on finding the Save button.
        cx.subscribe_in(&steamgriddb_key, window, |view, input, event, _, cx| {
            if let InputEvent::Change = event {
                view.config.steamgriddb_key = input.read(cx).value().trim().to_owned();
                view.message = match view.config.save() {
                    Ok(()) if view.config.steamgriddb_key.is_empty() => "API key removed.".into(),
                    Ok(()) => "API key saved.".into(),
                    Err(error) => format!("Could not save API key: {error}"),
                };
                cx.notify();
            }
        })
        .detach();
        let query = cx.new(|cx| InputState::new(window, cx).placeholder("Search games"));
        cx.subscribe_in(&query, window, |view, _, event, _, cx| match event {
            InputEvent::PressEnter { .. } => {
                if let Some(&i) = view.visible(cx).first() {
                    view.selected = Some(i);
                    view.play(i, cx);
                }
            }
            InputEvent::Change => cx.notify(),
            _ => {}
        })
        .detach();
        cx.observe_window_activation(window, |view, window, _| view.active = window.is_window_active()).detach();
        let pad = gamepad::spawn();
        cx.spawn_in(window, async move |this, cx| {
            loop {
                cx.background_executor().timer(std::time::Duration::from_millis(16)).await;
                let actions = std::mem::take(&mut *pad.lock().unwrap());
                if !actions.is_empty()
                    && this.update_in(cx, |view, window, cx| actions.into_iter().for_each(|a| view.on_pad(a, window, cx))).is_err()
                {
                    return;
                }
            }
        })
        .detach();

        let mut hub = Self {
            query,
            roms_dir,
            retroarch_cmd,
            cores_dir,
            steamgriddb_key,
            systems: emu::systems(),
            games: vec![],
            config,
            view: View::Home,
            system: None,
            selected: None,
            sidebar_open: true,
            busy: false,
            focus: Focus::Menu(0),
            details: None,
            pad_used: false,
            active: true,
            running: None,
            columns: 1,
            grid_scroll: UniformListScrollHandle::new(),
            visible_cache: vec![],
            covers: vec![],
            cover_pass: 0,
            home_scroll: [ScrollHandle::new(), ScrollHandle::new()],
            home_page_scroll: ScrollHandle::new(),
            settings_scroll: ScrollHandle::new(),
            history: emu::load_history(),
            message: String::new(),
            appearance,
        };
        hub.rescan(cx);
        hub
    }

    /// Indexes into `games` that pass the system filter and search box.
    fn visible(&self, cx: &App) -> Vec<usize> {
        let query = self.query.read(cx).value().to_lowercase();
        (0..self.games.len())
            .filter(|&i| {
                let g = &self.games[i];
                self.system.as_deref().is_none_or(|s| s == g.system) && g.name.to_lowercase().contains(&query)
            })
            .collect()
    }

    fn rescan(&mut self, cx: &mut Context<Self>) {
        self.games = emu::scan(&self.config.roms_dir, &self.systems);
        self.refresh_covers();
        self.selected = None;
        if self.system.as_deref().is_some_and(|s| !self.games.iter().any(|g| g.system == s)) {
            self.system = None;
        }
        if self.games.is_empty() {
            self.message = "No games found. Open Settings, create the ROM folders, and put your games in them.".into();
        }
        self.fetch_covers(cx);
        cx.notify();
    }

    /// Downloads missing covers one by one in the background, redrawing as each arrives.
    fn refresh_covers(&mut self) {
        self.covers = self.games.iter().map(|g| Some(emu::cover_path(g)).filter(|p| p.exists())).collect();
    }

    fn fetch_covers(&mut self, cx: &mut Context<Self>) {
        self.cover_pass += 1;
        let pass = self.cover_pass;
        // Only games still missing a cover; the rest are already on disk.
        let missing: Vec<(usize, Game)> =
            self.games.iter().enumerate().filter(|(i, _)| self.covers[*i].is_none()).map(|(i, g)| (i, g.clone())).collect();
        let recent: Vec<Game> = self.recent().into_iter().map(|i| self.games[i].clone()).collect();
        let cached: Vec<std::path::PathBuf> = self.covers.iter().flatten().cloned().collect();
        let key = self.config.steamgriddb_key.clone();
        cx.spawn(async move |this, cx| {
            // 8 downloads at a time, redrawing once per batch.
            for batch in missing.chunks(8) {
                let tasks: Vec<_> = batch
                    .iter()
                    .cloned()
                    .map(|(index, game)| {
                        let key = key.clone();
                        cx.background_executor().spawn(async move { (index, game.path.clone(), emu::fetch_cover(&game, &key).then(|| emu::cover_path(&game))) })
                    })
                    .collect();
                let mut got = vec![];
                for task in tasks {
                    got.push(task.await);
                }
                let alive = this.update(cx, |view, cx| {
                    if view.cover_pass != pass {
                        return false;
                    }
                    for (index, path, cover) in got {
                        if let Some(cover) = cover.filter(|_| view.games.get(index).is_some_and(|g| g.path == path)) {
                            view.covers[index] = Some(cover);
                        }
                    }
                    cx.notify();
                    true
                });
                if !alive.unwrap_or(false) {
                    return;
                }
            }
            // One-time cleanup of full-size covers from older versions; already-small ones are skipped by size.
            cx.background_executor().spawn(async move { cached.iter().for_each(|p| emu::shrink_cached_cover(p)) }).await;
            for game in recent {
                let got = cx.background_executor().spawn(async move { emu::fetch_snap(&game) }).await;
                if got && this.update(cx, |_, cx| cx.notify()).is_err() {
                    return;
                }
            }
        })
        .detach();
    }

    /// True (and says so) while a game or RetroArch that EmuHub started is still open.
    fn game_running(&mut self) -> bool {
        let open = self.running.as_mut().is_some_and(|r| r.is_open());
        if open {
            self.message = "A game is already running. Close it first (Share + Options).".into();
        } else {
            self.running = None;
        }
        open
    }

    fn play(&mut self, index: usize, cx: &mut Context<Self>) {
        if self.game_running() {
            cx.notify();
            return;
        }
        let game = &self.games[index];
        self.message = match emu::launch(&self.config, game) {
            Ok(running) => {
                self.running = Some(running);
                format!("Launched {}", game.name)
            }
            Err(error) => error,
        };
        self.history = emu::load_history();
        let game = game.clone();
        cx.spawn(async move |this, cx| {
            if cx.background_executor().spawn(async move { emu::fetch_snap(&game) }).await {
                let _ = this.update(cx, |_, cx| cx.notify());
            }
        })
        .detach();
        cx.notify();
    }

    /// Side-menu entries in order: All games, each system that has games, Settings.
    fn menu(&self) -> Vec<MenuItem> {
        let mut items = vec![MenuItem::Home, MenuItem::Library(None)];
        items.extend(
            self.systems
                .iter()
                .filter(|s| self.games.iter().any(|g| g.system == s.id))
                .map(|s| MenuItem::Library(Some(s.id.clone()))),
        );
        items.push(MenuItem::Settings);
        items
    }

    /// The menu entry for the page on screen.
    fn current_menu(&self) -> usize {
        let here = match self.view {
            View::Home => MenuItem::Home,
            View::Settings => MenuItem::Settings,
            View::Library => MenuItem::Library(self.system.clone()),
        };
        self.menu().iter().position(|m| *m == here).unwrap_or(0)
    }

    fn show(&mut self, item: &MenuItem) {
        match item {
            MenuItem::Home => self.view = View::Home,
            MenuItem::Settings => self.view = View::Settings,
            MenuItem::Library(system) => {
                if self.view != View::Library || self.system != *system {
                    self.selected = None;
                }
                self.view = View::Library;
                self.system = system.clone();
            }
        }
    }

    fn on_pad(&mut self, action: Pad, window: &mut Window, cx: &mut Context<Self>) {
        // While a game is in front, the only thing EmuHub listens for is the close combo.
        if action == Pad::CloseGame {
            if !gamepad::app_in_front() {
                if let Some(running) = &self.running {
                    running.close();
                }
            }
            return;
        }
        if !self.active || !gamepad::app_in_front() {
            return;
        }
        self.pad_used = true;
        if self.details.is_some() {
            self.details_pad(action, cx);
            cx.notify();
            return;
        }
        let menu = self.menu();
        match (self.focus, action) {
            (_, Pad::CloseGame) => {} // handled above
            (_, Pad::PrevTab | Pad::NextTab) => {
                let tabs = menu.len() - 1; // Settings isn't a tab
                let at = self.current_menu().min(tabs - 1);
                let next = if action == Pad::NextTab { (at + 1) % tabs } else { (at + tabs - 1) % tabs };
                self.show(&menu[next]);
                self.focus = Focus::Menu(next);
            }
            // Side menu: up/down previews each page, X or Right goes into it.
            (Focus::Menu(i), Pad::Up | Pad::Down) => {
                let i = if action == Pad::Up { i.saturating_sub(1) } else { (i + 1).min(menu.len() - 1) };
                self.show(&menu[i]);
                self.focus = Focus::Menu(i);
            }
            (Focus::Menu(i), Pad::Confirm | Pad::Right) => {
                self.show(&menu[i]);
                if menu[i] == MenuItem::Settings {
                    self.focus = Focus::Settings(0);
                    self.scroll_settings_to(0);
                } else if menu[i] == MenuItem::Home {
                    self.enter_home();
                } else if !self.visible(cx).is_empty() {
                    self.focus = Focus::Grid;
                    self.move_selection(0, cx);
                }
            }
            (Focus::Menu(_), _) => {}
            (Focus::Home(row, col), _) => self.home_pad(row, col, action, cx),
            // Game grid: Left from the first column, or O, returns to the menu.
            (Focus::Grid, Pad::Back) => self.focus = Focus::Menu(self.current_menu()),
            (Focus::Grid, Pad::Left) if self.grid_position(cx).is_none_or(|p| p % self.columns == 0) => {
                self.focus = Focus::Menu(self.current_menu())
            }
            (Focus::Grid, Pad::Confirm) => match self.selected {
                Some(i) => self.play(i, cx),
                None => self.move_selection(0, cx),
            },
            (Focus::Grid, Pad::Options) => {
                if let Some(i) = self.selected {
                    self.open_details(i, cx)
                }
            }
            (Focus::Grid, Pad::Left) => self.move_selection(-1, cx),
            (Focus::Grid, Pad::Right) => self.move_selection(1, cx),
            (Focus::Grid, Pad::Up) => self.move_selection(-(self.columns as isize), cx),
            (Focus::Grid, Pad::Down) => self.move_selection(self.columns as isize, cx),
            // Settings: up/down through the buttons, X presses, O or Left returns to the menu.
            (Focus::Settings(_), Pad::Back | Pad::Left) => self.focus = Focus::Menu(self.current_menu()),
            (Focus::Settings(j), Pad::Up | Pad::Down) => {
                let j = if action == Pad::Up { j.saturating_sub(1) } else { (j + 1).min(settings_items().len() - 1) };
                self.focus = Focus::Settings(j);
                self.scroll_settings_to(j);
            }
            (Focus::Settings(j), Pad::Confirm) => self.apply_setting(settings_items()[j], window, cx),
            (Focus::Settings(_), Pad::Right | Pad::Options) => {}
        }
        cx.notify();
    }

    fn open_details(&mut self, game: usize, cx: &mut Context<Self>) {
        self.selected = Some(game);
        self.details = Some(Details { game, candidates: None, loading: false, focus: 0, scroll: ScrollHandle::new() });
        cx.notify();
    }

    /// Controller on the game page: any direction steps through the buttons and cover choices.
    fn details_pad(&mut self, action: Pad, cx: &mut Context<Self>) {
        let Some(details) = &mut self.details else { return };
        let count = DETAIL_BUTTONS + details.candidates.as_ref().map_or(0, Vec::len);
        let focus = details.focus;
        // Buttons are 0..3 in one row; covers follow in rows of CHOICE_COLUMNS.
        let next = match action {
            Pad::Back | Pad::Options => {
                self.details = None;
                return;
            }
            Pad::Confirm => return self.details_action(focus, cx),
            Pad::PrevTab | Pad::NextTab | Pad::CloseGame => return,
            Pad::Left => focus.saturating_sub(1),
            Pad::Right => (focus + 1).min(count - 1),
            Pad::Up if focus < DETAIL_BUTTONS => focus,
            Pad::Up if focus < DETAIL_BUTTONS + CHOICE_COLUMNS => 1, // first row of covers → Change cover
            Pad::Up => focus - CHOICE_COLUMNS,
            Pad::Down if focus < DETAIL_BUTTONS => DETAIL_BUTTONS.min(count - 1),
            Pad::Down => (focus + CHOICE_COLUMNS).min(count - 1),
        };
        details.focus = next;
        if next >= DETAIL_BUTTONS {
            details.scroll.scroll_to_item(next - DETAIL_BUTTONS);
        }
    }

    /// 0 = Play, 1 = Change cover, 2 = Back, 3.. = pick that cover.
    fn details_action(&mut self, item: usize, cx: &mut Context<Self>) {
        let Some(details) = &mut self.details else { return };
        let game = self.games[details.game].clone();
        match item {
            0 => {
                let index = details.game;
                self.details = None;
                self.play(index, cx);
            }
            1 if !details.loading => {
                details.loading = true;
                details.candidates = None;
                let key = self.config.steamgriddb_key.clone();
                cx.spawn(async move |this, cx| {
                    let found = cx.background_executor().spawn(async move { emu::cover_candidates(&game, &key) }).await;
                    let _ = this.update(cx, |view, cx| {
                        if let Some(details) = &mut view.details {
                            details.loading = false;
                            details.candidates = Some(found);
                        }
                        cx.notify();
                    });
                })
                .detach();
            }
            1 => {}
            2 => self.details = None,
            n => {
                let Some(choice) = details.candidates.as_ref().and_then(|c| c.get(n - DETAIL_BUTTONS)) else { return };
                let url = choice.url.clone();
                self.run("Downloading cover…", move |_| emu::set_cover(&game, &url).map(|()| "Cover updated.".into()), cx);
            }
        }
        cx.notify();
    }

    fn details_window(&self, details: &Details, cx: &mut Context<Self>) -> Stateful<Div> {
        let game = &self.games[details.game];
        let cover = emu::cover_path(game);
        let art = if cover.exists() {
            img(cover).size_full().object_fit(ObjectFit::Cover).into_any_element()
        } else {
            tile(&game.name, 160.).size_full().rounded_none().into_any_element()
        };
        let focused = |i: usize| details.focus == i;
        // Round icon buttons with a label underneath; Play is filled with the accent color.
        let mut buttons = div().flex().gap_6().pt_4();
        for (i, (id, name, label)) in
            [("detail-play", "play", "Play"), ("detail-cover", "image", "Change cover"), ("detail-close", "arrow-left", "Back")].into_iter().enumerate()
        {
            let (bg, fg) = if i == 0 { (rgb(self.appearance.accent), rgb(0x0c1417)) } else { (rgba(0xffffff1a), rgb(TEXT)) };
            let circle = self
                .ring(div().w(px(64.)).h(px(64.)).rounded_full().flex().items_center().justify_center().bg(bg), focused(i))
                .child(icon(name, 26., fg));
            buttons = buttons.child(
                div()
                    .id(id)
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap_2()
                    .cursor_pointer()
                    .when(i == 1 && details.loading, |d| d.opacity(0.5))
                    .hover(|style| style.opacity(0.85))
                    .on_click(cx.listener(move |view, _, _, cx| view.details_action(i, cx)))
                    .child(circle)
                    .child(div().text_xs().text_color(rgb(MUTED)).child(label)),
            );
        }
        let played = self.history.get(&game.path).map(|&t| format!("Last played {}", emu::ago(t)));

        let mut panel = div()
            .id("details-panel")
            .size_full()
            .overflow_y_scroll()
            .px_12()
            .py_10()
            .flex()
            .flex_col()
            .gap_6()
            .child(
                div()
                    .flex()
                    .items_end()
                    .gap_10()
                    .child(div().w(px(240.)).h(px(360.)).flex_none().rounded_lg().overflow_hidden().shadow_lg().child(art))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap_2()
                            .child(heading(system_name(&self.systems, &game.system)))
                            .child(div().text_size(px(40.)).font_weight(FontWeight::SEMIBOLD).child(game.name.clone()))
                            .children(played.map(|p| div().flex().items_center().gap_2().text_color(rgb(MUTED)).child(icon("history", 16., rgb(MUTED))).child(p)))
                            .child(div().text_xs().text_color(rgb(MUTED)).child(game.path.display().to_string()))
                            .child(buttons),
                    ),
            );

        if details.loading {
            panel = panel.child(div().text_color(rgb(MUTED)).child("Finding covers…"));
        } else if let Some(candidates) = &details.candidates {
            let mut choices = div()
                .id("cover-choices")
                .w(px(740.))
                .max_h(px(290.))
                .overflow_y_scroll()
                .track_scroll(&details.scroll)
                .flex()
                .flex_wrap()
                .gap_4();
            for (n, choice) in candidates.iter().enumerate() {
                let item = DETAIL_BUTTONS + n;
                choices = choices.child(
                    div()
                        .id(("cover-choice", n))
                        .w(px(110.))
                        .flex()
                        .flex_col()
                        .gap_1()
                        .cursor_pointer()
                        .on_click(cx.listener(move |view, _, _, cx| view.details_action(item, cx)))
                        .child(
                            self.ring(div().w(px(110.)).h(px(165.)).rounded_md().overflow_hidden(), focused(item))
                                .child(img(choice.preview.clone()).size_full().object_fit(ObjectFit::Cover)),
                        )
                        .child(div().text_xs().text_color(rgb(MUTED)).line_clamp(2).child(choice.label.clone())),
                );
            }
            panel = panel.child(heading("Choose a cover")).child(if candidates.is_empty() {
                div().text_color(rgb(MUTED)).child("No other covers found. Add a SteamGridDB key in Settings for more choices.").into_any_element()
            } else {
                choices.into_any_element()
            });
        }

        // Full page between the title bar and the hints bar, over a dimmed copy of the cover.
        let cover = emu::cover_path(game);
        div()
            .id("details")
            .relative()
            .flex_1()
            .min_h_0()
            .overflow_hidden()
            .bg(appearance::Appearance { glass: false, ..self.appearance }.fill(0x101e23))
            .when(cover.exists(), |d| {
                d.child(div().absolute().top_0().left_0().size_full().opacity(0.12).child(img(cover).size_full().object_fit(ObjectFit::Cover)))
            })
            .child(panel)
    }

    /// Position of the selected game within the visible list.
    fn grid_position(&self, cx: &App) -> Option<usize> {
        let selected = self.selected?;
        self.visible(cx).iter().position(|&v| v == selected)
    }

    /// Scrolls Settings so the card holding button `j` is visible (the page's children are
    /// title, then a heading and a card per section; see settings_page).
    fn scroll_settings_to(&self, j: usize) {
        let child = match settings_items()[j] {
            _ if j == 0 => 0, // first button: show the page title too
            Setting::EmulationFolder | Setting::RomFolders | Setting::RetroArch | Setting::Cores => 2,
            Setting::EmulatorFolder(_) | Setting::PickEmulator(_) | Setting::EmulatorSite(_) => 4,
            Setting::Glass(_) | Setting::Palette(_) => 6,
        };
        self.settings_scroll.scroll_to_item(child);
    }

    /// What each Settings button does, for both mouse clicks and the controller.
    fn apply_setting(&mut self, setting: Setting, window: &mut Window, cx: &mut Context<Self>) {
        match setting {
            Setting::EmulationFolder => {
                let picked = cx.prompt_for_paths(PathPromptOptions {
                    files: false,
                    directories: true,
                    multiple: false,
                    prompt: Some("Choose where your Emulation folder goes".into()),
                });
                cx.spawn_in(window, async move |this, cx| {
                    if let Ok(Ok(Some(mut paths))) = picked.await {
                        let _ = this.update_in(cx, |view, window, cx| {
                            view.config.roms_dir = emu::roms_dir_for(&paths.remove(0));
                            let roms = view.config.roms_dir.display().to_string();
                            view.roms_dir.update(cx, |input, cx| input.set_value(roms, window, cx));
                            view.message = match emu::create_folders(&view.config).and_then(|()| view.config.save()) {
                                Ok(()) => format!("Emulation folder: {}", emu::emulation_folder(&view.config).display()),
                                Err(error) => error,
                            };
                            view.rescan(cx);
                        });
                    }
                })
                .detach();
            }
            Setting::RomFolders if self.config.roms_dir.is_dir() => {
                self.message = emu::create_folders(&self.config)
                    .and_then(|()| emu::open_folder(&self.config.roms_dir))
                    .err()
                    .unwrap_or_default();
            }
            Setting::RomFolders => {
                self.message = match emu::create_folders(&self.config) {
                    Ok(()) => format!("Created folders in {}", self.config.roms_dir.display()),
                    Err(error) => error,
                };
                self.rescan(cx);
            }
            Setting::RetroArch if emu::retroarch_installed(&self.config) => {
                if !self.game_running() {
                    match emu::open_retroarch(&self.config) {
                        Ok(running) => self.running = Some(running),
                        Err(error) => self.message = error,
                    }
                }
            }
            Setting::RetroArch => {
                self.run("Installing RetroArch…", |_| emu::install_retroarch().map(|()| "RetroArch installed.".into()), cx)
            }
            Setting::Cores => {
                if emu::core_status(&self.config).0 > 0 {
                    self.run("Downloading cores…", |config| emu::install_cores(&config).map(|n| format!("Installed {n} new cores.")), cx)
                }
            }
            Setting::Glass(glass) => {
                self.appearance.glass = glass;
                self.save_appearance(window, cx);
            }
            Setting::PickEmulator(i) => {
                let emulator = &emu::EMULATORS[i];
                let picked = cx.prompt_for_paths(PathPromptOptions {
                    files: true,
                    directories: false,
                    multiple: false,
                    prompt: Some(format!("Choose the {} program", emulator.name).into()),
                });
                cx.spawn(async move |this, cx| {
                    if let Ok(Ok(Some(mut paths))) = picked.await {
                        let _ = this.update(cx, |view, cx| {
                            view.config.emulators.insert(emulator.id.to_owned(), paths.remove(0));
                            view.message = match view.config.save() {
                                Ok(()) => format!("{} set.", emulator.name),
                                Err(error) => error,
                            };
                            cx.notify();
                        });
                    }
                })
                .detach();
            }
            Setting::EmulatorFolder(i) => {
                let dir = emu::emulator_dir(&self.config, &emu::EMULATORS[i]);
                self.message = std::fs::create_dir_all(&dir)
                    .map_err(|e| e.to_string())
                    .and_then(|()| emu::open_folder(&dir))
                    .err()
                    .unwrap_or_default();
            }
            Setting::EmulatorSite(i) => self.message = emu::open_url(emu::EMULATORS[i].site).err().unwrap_or_default(),
            Setting::Palette(i) => {
                let (_, surface, accent) = PALETTES[i];
                self.appearance.surface = surface;
                self.appearance.accent = accent;
                self.save_appearance(window, cx);
            }
        }
        cx.notify();
    }

    /// Accent outline around whatever the controller is on.
    fn ring<E: Styled>(&self, element: E, on: bool) -> E {
        let color: Hsla = if on && self.pad_used { rgb(self.appearance.accent).into() } else { rgba(0x00000000).into() };
        element.border_2().border_color(color)
    }

    /// A Settings button wrapped in its focus ring.
    fn setting_button(&self, setting: Setting, button: Button, cx: &mut Context<Self>) -> Div {
        let focused = self.focus == Focus::Settings(settings_items().iter().position(|s| *s == setting).unwrap());
        self.ring(div().rounded_md(), focused)
            .child(button.on_click(cx.listener(move |view, _, window, cx| view.apply_setting(setting, window, cx))))
    }

    /// Moves the selection through the visible games, starting at the first one if nothing is selected.
    fn move_selection(&mut self, by: isize, cx: &mut Context<Self>) {
        let visible = self.visible(cx);
        if visible.is_empty() {
            return;
        }
        let pos = match self.selected.and_then(|i| visible.iter().position(|&v| v == i)) {
            Some(pos) => (pos as isize + by).clamp(0, visible.len() as isize - 1) as usize,
            None => 0,
        };
        self.selected = Some(visible[pos]);
        self.grid_scroll.scroll_to_item(pos / self.columns.max(1), ScrollStrategy::Center);
    }

    /// Runs a slow job (install, download) off the UI thread and shows its result.
    fn run(&mut self, label: &str, job: impl FnOnce(Config) -> Result<String, String> + Send + 'static, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        self.busy = true;
        self.message = label.into();
        let config = self.config.clone();
        cx.spawn(async move |this, cx| {
            let result = cx.background_executor().spawn(async move { job(config) }).await;
            let _ = this.update(cx, |view, cx| {
                view.busy = false;
                view.message = result.unwrap_or_else(|e| e);
                view.refresh_covers();
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn save_settings(&mut self, cx: &mut Context<Self>) {
        let read = |input: &Entity<InputState>| input.read(cx).value().trim().to_owned();
        self.config.roms_dir = read(&self.roms_dir).into();
        self.config.retroarch_cmd = read(&self.retroarch_cmd).split_whitespace().map(String::from).collect();
        self.config.cores_dir = read(&self.cores_dir).into();
        self.config.steamgriddb_key = read(&self.steamgriddb_key);
        self.message = match self.config.save() {
            Ok(()) => "Settings saved.".into(),
            Err(error) => error,
        };
        self.rescan(cx);
    }

    // ---- navigation -------------------------------------------------------

    fn nav_row(&self, id: impl Into<ElementId>, selected: bool) -> Stateful<Div> {
        div()
            .id(id)
            .flex()
            .items_center()
            .gap_3()
            .px_3()
            .h(px(40.))
            .flex_none()
            .rounded_md()
            .cursor_pointer()
            .text_size(px(15.))
            .text_color(rgb(if selected { 0xf5f3ee } else { 0xc3cdd1 }))
            .bg(if selected { rgba((self.appearance.accent << 8) | 0x30) } else { rgba(0x00000000) })
            .hover(|style| style.bg(rgba(0xffffff12)))
    }

    fn sidebar(&self, cx: &mut Context<Self>) -> Stateful<Div> {
        let tone = |selected: bool| rgb(if selected { 0xf5f3ee } else { MUTED });
        let all = self.view == View::Library && self.system.is_none();
        let settings = self.view == View::Settings;
        let menu_len = self.menu().len();
        let focused = |i: usize| self.focus == Focus::Menu(i);
        let home = self.view == View::Home;
        let mut nav = div()
            .flex()
            .flex_col()
            .gap_1()
            .child(
                self.ring(self.nav_row("nav-home", home), focused(0))
                    .child(icon("home", 18., tone(home)))
                    .child("Home")
                    .on_click(cx.listener(|view, _, _, cx| {
                        view.view = View::Home;
                        cx.notify();
                    })),
            )
            .child(
                self.ring(self.nav_row("nav-all", all), focused(1))
                    .child(icon("library", 18., tone(all)))
                    .child(div().flex_1().child("All games"))
                    .child(div().text_xs().text_color(rgb(MUTED)).child(self.games.len().to_string()))
                    .on_click(cx.listener(|view, _, _, cx| {
                        view.view = View::Library;
                        view.system = None;
                        cx.notify();
                    })),
            )
            .child(div().px_3().pt_3().pb_1().child(heading("Your Library")));
        let mut row = 1;
        for system in &self.systems {
            let count = self.games.iter().filter(|g| g.system == system.id).count();
            if count == 0 {
                continue;
            }
            row += 1;
            let selected = self.view == View::Library && self.system.as_deref() == Some(system.id.as_str());
            let pick = system.id.clone();
            nav = nav.child(
                self.ring(self.nav_row(SharedString::from(format!("sys-{}", system.id)), selected), focused(row))
                    .child(tile(&system.name, 24.))
                    .child(div().flex_1().truncate().child(system.name.clone()))
                    .child(div().text_xs().text_color(rgb(MUTED)).child(count.to_string()))
                    .on_click(cx.listener(move |view, _, _, cx| {
                        view.view = View::Library;
                        view.system = Some(pick.clone());
                        cx.notify();
                    })),
            );
        }
        div()
            .id("sidebar")
            .w(px(256.))
            .h_full()
            .flex_shrink_0()
            .overflow_y_scroll()
            .bg(self.appearance.fill(0x081012))
            .p_3()
            .flex()
            .flex_col()
            .justify_between()
            .child(nav)
            .child(
                self.ring(self.nav_row("nav-settings", settings), focused(menu_len - 1))
                    .child(icon("settings", 18., tone(settings)))
                    .child("Settings")
                    .on_click(cx.listener(|view, _, _, cx| {
                        view.view = View::Settings;
                        cx.notify();
                    })),
            )
    }

    // ---- pages ------------------------------------------------------------

    fn library_page(&self, visible: &[usize], cx: &mut Context<Self>) -> Stateful<Div> {
        let title = match &self.system {
            Some(id) => system_name(&self.systems, id).to_owned(),
            None => "All games".to_owned(),
        };
        let rows = visible.len().div_ceil(self.columns.max(1));
        let grid = if visible.is_empty() {
            div().px_6().text_color(rgb(MUTED)).child("No games here.").into_any_element()
        } else {
            uniform_list("grid", rows, cx.processor(|view: &mut Hub, range: std::ops::Range<usize>, _, cx| range.map(|row| view.grid_row(row, cx)).collect::<Vec<_>>()))
                .flex_1()
                .track_scroll(self.grid_scroll.clone())
                .into_any_element()
        };
        div()
            .id("library")
            .size_full()
            .bg(self.appearance.fill(0x0c1417))
            .flex()
            .flex_col()
            .child(
                div()
                    .flex()
                    .items_end()
                    .gap_6()
                    .p_6()
                    .child(tile(&title, 128.).rounded_lg())
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_2()
                            .child(heading("Library"))
                            .child(div().text_size(px(34.)).font_weight(FontWeight::SEMIBOLD).child(title))
                            .child(div().text_color(rgb(MUTED)).child(format!(
                                "{} game{}",
                                visible.len(),
                                if visible.len() == 1 { "" } else { "s" }
                            )))
                            .child(
                                div().pt_2().child(
                                    Button::new("rescan")
                                        .label("Rescan")
                                        .on_click(cx.listener(|view, _, _, cx| view.rescan(cx))),
                                ),
                            ),
                    ),
            )
            .child(div().px_6().pb_4().child(Input::new(&self.query)))
            .child(grid)
    }

    fn settings_page(&self, cx: &mut Context<Self>) -> Stateful<Div> {
        let paths = card()
            .child(setting_row("ROMs folder", "One subfolder per system, EmuDeck style (roms/snes, roms/gba…).").child(div().w(px(360.)).child(Input::new(&self.roms_dir))))
            .child(setting_row("RetroArch command", "Program and arguments, separated by spaces.").child(div().w(px(360.)).child(Input::new(&self.retroarch_cmd))))
            .child(setting_row("Cores folder", "Where RetroArch keeps its cores.").child(div().w(px(360.)).child(Input::new(&self.cores_dir))))
            .child(setting_row("SteamGridDB API key", "Optional. Fills in covers libretro doesn't have. steamgriddb.com → Preferences → API.").child(div().w(px(360.)).child(Input::new(&self.steamgriddb_key).mask_toggle())))
            .child(setting_row("Save changes", "").child(
                Button::new("save").primary().label("Save").on_click(cx.listener(|view, _, _, cx| view.save_settings(cx))),
            ));

        let folders_exist = self.config.roms_dir.is_dir();
        let retroarch = emu::retroarch_installed(&self.config);
        let (missing, total) = emu::core_status(&self.config);
        let setup = card()
            .child(
                setting_row(
                    "Emulation folder",
                    &format!("{}. Holds roms, bios and tools. Pick an existing copy, or any folder to make a new one in.", emu::emulation_folder(&self.config).display()),
                )
                .child(self.setting_button(Setting::EmulationFolder, Button::new("emulation-folder").label("Choose…"), cx)),
            )
            .child(
                if folders_exist {
                    setting_row("ROM folders", "Put each game in its system's folder, then Rescan. Also adds any missing bios and tools folders.")
                } else {
                    setting_row("ROM folders", "Creates EmuDeck's folders: one per system, plus bios and tools.")
                }
                .child(self.setting_button(
                    Setting::RomFolders,
                    Button::new("rom-folders").label(if folders_exist { "Open" } else { "Create" }),
                    cx,
                )),
            )
            .child(
                if retroarch {
                    setting_row("RetroArch", "Installed. Open it to change its own settings.")
                } else {
                    setting_row("RetroArch", "Not installed. Installs with winget, Homebrew or Flatpak.")
                }
                .child(self.setting_button(
                    Setting::RetroArch,
                    Button::new("retroarch").label(if retroarch { "Open" } else { "Install" }).disabled(self.busy && !retroarch),
                    cx,
                )),
            )
            .child(
                setting_row(
                    "Cores",
                    &if missing == 0 {
                        format!("All {total} cores installed.")
                    } else {
                        format!("{missing} of {total} cores missing. Downloads them from libretro.")
                    },
                )
                .child(self.setting_button(
                    Setting::Cores,
                    Button::new("cores").label(if missing == 0 { "Installed" } else { "Install" }).disabled(self.busy || missing == 0),
                    cx,
                )),
            );

        let mut emulators = card();
        for (i, emulator) in emu::EMULATORS.iter().enumerate() {
            let systems: Vec<&str> =
                self.systems.iter().filter(|s| s.standalone == emulator.id).map(|s| s.name.as_str()).collect();
            let note = match emu::find_emulator(&self.config, emulator) {
                Some(path) => format!("Found: {}", path.display()),
                None => format!("Not found. Unzip it into {}, or choose its program.", emu::emulator_dir(&self.config, emulator).display()),
            };
            emulators = emulators.child(
                setting_row(&format!("{} ({})", emulator.name, systems.join(", ")), &note).child(
                    div()
                        .flex()
                        .gap_2()
                        .child(self.setting_button(Setting::EmulatorFolder(i), Button::new(("emu-folder", i)).label("Open folder"), cx))
                        .child(self.setting_button(Setting::PickEmulator(i), Button::new(("pick-emu", i)).label("Choose…"), cx))
                        .child(self.setting_button(Setting::EmulatorSite(i), Button::new(("emu-site", i)).label("Website"), cx)),
                ),
            );
        }

        let mut modes = div().flex().gap_2();
        for (label, glass) in [("Solid", false), ("Glass", true)] {
            modes = modes.child(self.setting_button(
                Setting::Glass(glass),
                Button::new(label).label(label).selected(self.appearance.glass == glass),
                cx,
            ));
        }
        let mut palettes = div().flex().flex_wrap().justify_end().gap_2();
        for (i, (name, surface, _)) in PALETTES.into_iter().enumerate() {
            palettes = palettes.child(self.setting_button(
                Setting::Palette(i),
                Button::new(name).label(name).selected(self.appearance.surface == surface),
                cx,
            ));
        }
        let looks = card()
            .child(setting_row("Window material", "Glass blurs the desktop behind the window (Windows and macOS).").child(modes))
            .child(setting_row("Theme", "Preset colors for the whole window.").child(palettes));

        div()
            .id("settings-page")
            .size_full()
            .overflow_y_scroll()
            .track_scroll(&self.settings_scroll)
            .bg(self.appearance.fill(0x0c1417))
            .p_8()
            .flex()
            .flex_col()
            .gap_4()
            .child(div().text_size(px(34.)).font_weight(FontWeight::SEMIBOLD).child("Settings"))
            // Controller-reachable sections first; Paths needs a keyboard anyway.
            .child(div().pt_4().child(heading("Setup")))
            .child(setup)
            .child(div().pt_4().child(heading("Emulators")))
            .child(emulators)
            .child(div().pt_4().child(heading("Appearance")))
            .child(looks)
            .child(div().pt_4().child(heading("Paths")))
            .child(paths)
    }

    fn save_appearance(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.appearance.apply(window);
        self.appearance.apply_theme(cx);
        self.message = match self.appearance.save() {
            Ok(()) => "Appearance saved.".into(),
            Err(error) => error,
        };
        cx.notify();
    }

    // ---- home -------------------------------------------------------------

    /// Played games, most recent first (the Continue Playing row).
    fn recent(&self) -> Vec<usize> {
        let mut played: Vec<(u64, usize)> =
            self.games.iter().enumerate().filter_map(|(i, g)| self.history.get(&g.path).map(|&t| (t, i))).collect();
        played.sort_by(|a, b| b.0.cmp(&a.0));
        played.into_iter().take(12).map(|(_, i)| i).collect()
    }

    /// Systems that have games (the Systems row).
    fn home_systems(&self) -> Vec<&System> {
        self.systems.iter().filter(|s| self.games.iter().any(|g| g.system == s.id)).collect()
    }

    fn home_row_len(&self, row: usize) -> usize {
        if row == 0 { self.recent().len() } else { self.home_systems().len() }
    }

    /// First Home row with something in it, for entering Home with the controller.
    fn enter_home(&mut self) {
        if let Some(row) = (0..2).find(|&r| self.home_row_len(r) > 0) {
            self.focus = Focus::Home(row, 0);
            self.home_scroll[row].scroll_to_item(0);
            self.scroll_home_to(row);
        }
    }

    /// Scrolls Home so row `row` is visible; the top row also brings the hero back into view.
    fn scroll_home_to(&self, row: usize) {
        let has_recent = !self.recent().is_empty();
        let child = if row == 0 || !has_recent { 0 } else { 3 }; // hero, divider, Continue Playing, Systems
        self.home_page_scroll.scroll_to_item(child);
        if child == 3 {
            // scroll_to_item only reveals the row's top part when it's taller than what's left; Systems is the last row.
            self.home_page_scroll.scroll_to_bottom();
        }
    }

    fn home_pad(&mut self, row: usize, col: usize, action: Pad, cx: &mut Context<Self>) {
        let len = self.home_row_len(row);
        match action {
            Pad::Back => self.focus = Focus::Menu(self.current_menu()),
            Pad::Left if col == 0 => self.focus = Focus::Menu(self.current_menu()),
            Pad::Left | Pad::Right => {
                let col = if action == Pad::Left { col - 1 } else { (col + 1).min(len.saturating_sub(1)) };
                self.focus = Focus::Home(row, col);
                self.home_scroll[row].scroll_to_item(col);
            }
            Pad::Up | Pad::Down => {
                let other = 1 - row;
                let other_len = self.home_row_len(other);
                if other_len > 0 {
                    let col = col.min(other_len - 1);
                    self.focus = Focus::Home(other, col);
                    self.home_scroll[other].scroll_to_item(col);
                    self.scroll_home_to(other);
                }
            }
            Pad::Confirm if row == 0 => {
                if let Some(&game) = self.recent().get(col) {
                    self.play(game, cx);
                }
            }
            Pad::Options if row == 0 => {
                if let Some(&game) = self.recent().get(col) {
                    self.open_details(game, cx);
                }
            }
            Pad::Confirm => {
                if let Some(id) = self.home_systems().get(col).map(|s| s.id.clone()) {
                    self.show(&MenuItem::Library(Some(id)));
                    self.focus = Focus::Grid;
                    self.move_selection(0, cx);
                }
            }
            _ => {}
        }
    }

    /// Cover art, or the letter tile when there isn't any yet.
    fn cover(&self, index: usize, size: f32) -> AnyElement {
        match &self.covers[index] {
            Some(cover) => img(cover.clone()).size_full().object_fit(ObjectFit::Cover).into_any_element(),
            None => tile(&self.games[index].name, size).size_full().rounded_none().into_any_element(),
        }
    }

    /// One row of the game grid. Rows have a fixed height, which uniform_list needs.
    fn grid_row(&self, row: usize, cx: &mut Context<Self>) -> Div {
        let start = row * self.columns.max(1);
        let end = (start + self.columns.max(1)).min(self.visible_cache.len());
        let accent = self.appearance.accent;
        let mut cards = div().h(px(325.)).flex().gap_5().px_6();
        for &index in &self.visible_cache[start..end] {
            let game = &self.games[index];
            let selected = self.selected == Some(index);
            cards = cards.child(
                div()
                    .id(("game", index))
                    .w(px(150.))
                    .flex()
                    .flex_col()
                    .gap_2()
                    .cursor_pointer()
                    .on_mouse_down(
                        MouseButton::Right,
                        cx.listener(move |view, _: &MouseDownEvent, _, cx| {
                            view.focus = Focus::Grid;
                            view.open_details(index, cx);
                        }),
                    )
                    .on_click(cx.listener(move |view, event: &ClickEvent, _, cx| {
                        view.selected = Some(index);
                        view.focus = Focus::Grid;
                        if event.click_count() >= 2 {
                            view.play(index, cx);
                        }
                        cx.notify();
                    }))
                    .child(
                        div()
                            .w(px(150.))
                            .h(px(225.))
                            .flex_none()
                            .rounded_lg()
                            .overflow_hidden()
                            .border_2()
                            // Bright when the controller is in the grid, dim while it's in the side menu.
                            .border_color(match (selected, self.focus == Focus::Grid) {
                                (true, true) => rgb(accent).into(),
                                (true, false) => rgba((accent << 8) | 0x60),
                                _ => rgba(0xffffff14),
                            })
                            .child(self.cover(index, 150.)),
                    )
                    .child(div().h(px(40.)).text_size(px(14.)).line_clamp(2).child(game.name.clone()))
                    .child(div().text_xs().text_color(rgb(MUTED)).child(system_name(&self.systems, &game.system).to_owned())),
            );
        }
        cards
    }

    fn home_page(&self, cx: &mut Context<Self>) -> Stateful<Div> {
        let recent = self.recent();
        let hero_index = match self.focus {
            Focus::Home(0, c) => recent.get(c),
            _ => recent.first(),
        }
        .copied();
        let hero = hero_index.map(|i| &self.games[i]);

        // Hero: in-game screenshot on the left, title and facts on the right, like the old EmuHub's showcase.
        let hero_art = match hero {
            Some(game) if emu::snap_path(game).exists() => {
                img(emu::snap_path(game)).size_full().object_fit(ObjectFit::Cover).into_any_element()
            }
            // No screenshot yet: show the whole box, centered, rather than a stretched crop of it.
            Some(_) => div()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .bg(rgba(0x00000040))
                .child(div().w(px(180.)).h(px(270.)).rounded_lg().overflow_hidden().child(self.cover(hero_index.unwrap(), 180.)))
                .into_any_element(),
            None => tile("EmuHub", 280.).size_full().rounded_none().into_any_element(),
        };
        let (title, facts) = match hero {
            Some(game) => (
                game.name.clone(),
                div()
                    .flex()
                    .flex_wrap()
                    .justify_center()
                    .gap_x_5()
                    .gap_y_2()
                    .text_color(rgb(MUTED))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(icon("history", 16., rgb(MUTED)))
                            .child(self.history.get(&game.path).map(|&t| emu::ago(t)).unwrap_or_default()),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(icon("library", 16., rgb(MUTED)))
                            .child(system_name(&self.systems, &game.system).to_owned()),
                    ),
            ),
            None => (
                "Welcome to EmuHub".to_owned(),
                div().text_color(rgb(MUTED)).text_center().child("Games you play show up here. Pick one from All games to start."),
            ),
        };
        let hero = div()
            .flex()
            .h(px(300.))
            .flex_none()
            .child(div().w(relative(0.6)).h_full().overflow_hidden().child(hero_art))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .p_8()
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .gap_4()
                    .child(div().text_size(px(30.)).font_weight(FontWeight::SEMIBOLD).text_center().line_clamp(2).child(title))
                    .child(facts),
            );

        let accent = self.appearance.accent;
        let row_title = |text: &str, sub: Option<&str>| {
            let mut title = div().flex().gap_2().text_size(px(17.)).child(text.to_owned());
            if let Some(sub) = sub {
                title = title.child(div().text_color(rgb(MUTED)).truncate().child(format!("- {sub}")));
            }
            div().px_6().pt_5().pb_2().child(title)
        };

        let mut continue_row = div().id("recent-row").flex().gap_4().px_6().pb_2().overflow_x_scroll().track_scroll(&self.home_scroll[0]);
        for (col, &index) in recent.iter().enumerate() {
            continue_row = continue_row.child(
                self.ring(div().id(("recent", col)).w(px(120.)).h(px(180.)).flex_none().rounded_lg().overflow_hidden(), self.focus == Focus::Home(0, col))
                    .cursor_pointer()
                    .bg(rgba(0xffffff0a))
                    .child(self.cover(index, 120.))
                    .on_mouse_down(
                        MouseButton::Right,
                        cx.listener(move |view, _: &MouseDownEvent, _, cx| {
                            view.focus = Focus::Home(0, col);
                            view.open_details(index, cx);
                        }),
                    )
                    .on_click(cx.listener(move |view, event: &ClickEvent, _, cx| {
                        view.focus = Focus::Home(0, col);
                        view.selected = Some(index);
                        if event.click_count() >= 2 {
                            view.play(index, cx);
                        }
                        cx.notify();
                    })),
            );
        }

        let mut systems_row = div().id("systems-row").flex().gap_4().px_6().pb_6().overflow_x_scroll().track_scroll(&self.home_scroll[1]);
        for (col, system) in self.home_systems().into_iter().enumerate() {
            let count = self.games.iter().filter(|g| g.system == system.id).count();
            let id = system.id.clone();
            systems_row = systems_row.child(
                self.ring(div().id(("system", col)).w(px(150.)).h(px(110.)).flex_none().rounded_lg(), self.focus == Focus::Home(1, col))
                    .cursor_pointer()
                    .bg(rgba(0xffffff0a))
                    .hover(|style| style.bg(rgba(0xffffff14)))
                    .p_3()
                    .flex()
                    .flex_col()
                    .justify_between()
                    .child(tile(&system.name, 36.))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .child(div().truncate().child(system.name.clone()))
                            .child(div().text_xs().text_color(rgb(MUTED)).child(format!("{count} game{}", if count == 1 { "" } else { "s" }))),
                    )
                    .on_click(cx.listener(move |view, _, _, cx| {
                        view.show(&MenuItem::Library(Some(id.clone())));
                        cx.notify();
                    })),
            );
        }

        let focused_name = match self.focus {
            Focus::Home(0, c) => recent.get(c).map(|&i| self.games[i].name.as_str()),
            _ => None,
        };
        let mut page = div()
            .id("home")
            .size_full()
            .overflow_y_scroll()
            .track_scroll(&self.home_page_scroll)
            .bg(self.appearance.fill(0x0c1417))
            .flex()
            .flex_col()
            .child(hero)
            .child(div().h(px(1.)).bg(rgba((accent << 8) | 0x30)));
        if !recent.is_empty() {
            page = page.child(div().child(row_title("Continue Playing", focused_name)).child(continue_row));
        }
        page.child(div().child(row_title("Systems", None)).child(systems_row))
    }

    // ---- button hints -----------------------------------------------------

    /// What the controller buttons do right now, shown in the bottom bar.
    fn hints(&self) -> Vec<(Hint, &'static str)> {
        if self.details.is_some() {
            return vec![(Hint::Confirm, "Select"), (Hint::Back, "Back")];
        }
        match self.focus {
            Focus::Menu(_) => vec![(Hint::Confirm, "Open"), (Hint::Bumpers, "Switch system")],
            Focus::Grid | Focus::Home(0, _) => vec![(Hint::Confirm, "Play"), (Hint::Options, "Details"), (Hint::Back, "Back")],
            Focus::Home(..) => vec![(Hint::Confirm, "Open"), (Hint::Back, "Back")],
            Focus::Settings(_) => vec![(Hint::Confirm, "Select"), (Hint::Back, "Back")],
        }
    }

    fn hint_bar(&self) -> Div {
        let ps = gamepad::PLAYSTATION.load(std::sync::atomic::Ordering::Relaxed);
        let mut bar = div().flex_1().flex().items_center().justify_center().gap_6();
        for (hint, label) in self.hints() {
            let badge = div().h(px(24.)).min_w(px(24.)).px_1().rounded_full().border_1().border_color(rgba(0xffffff50)).flex().items_center().justify_center().text_xs();
            let badge = match (hint, ps) {
                (Hint::Confirm, true) => badge.child(icon("btn-cross", 14., rgb(TEXT))),
                (Hint::Back, true) => badge.child(icon("btn-circle", 14., rgb(TEXT))),
                (Hint::Confirm, false) => badge.child("A"),
                (Hint::Back, false) => badge.child("B"),
                (Hint::Options, _) => badge.child(icon("btn-menu", 14., rgb(TEXT))),
                (Hint::Bumpers, true) => badge.px_2().child("L1 R1"),
                (Hint::Bumpers, false) => badge.px_2().child("LB RB"),
            };
            bar = bar.child(div().flex().items_center().gap_2().child(badge).child(label));
        }
        bar
    }

    fn bottom_bar(&self) -> Div {
        let focus = self.selected.map(|i| &self.games[i]);
        let left = match focus {
            Some(game) => div()
                .w(px(300.))
                .flex()
                .items_center()
                .gap_3()
                .child(tile(&game.name, 44.))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .child(div().truncate().text_size(px(15.)).child(game.name.clone()))
                        .child(div().truncate().text_xs().text_color(rgb(MUTED)).child(system_name(&self.systems, &game.system).to_owned())),
                ),
            None => div().w(px(300.)).text_color(rgb(MUTED)).child("Nothing selected"),
        };
        div()
            .h(px(76.))
            .flex_shrink_0()
            .px_5()
            .bg(self.appearance.fill(0x101e23))
            .border_t_1()
            .border_color(rgba(0xffffff14))
            .flex()
            .items_center()
            .gap_4()
            .child(left)
            .child(self.hint_bar())
            .child(div().w(px(300.)).text_xs().text_right().text_color(rgb(MUTED)).child(if self.message.is_empty() {
                format!("{} games", self.games.len())
            } else {
                self.message.clone()
            }))
    }
}

impl Render for Hub {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let on_page = match self.focus {
            Focus::Menu(i) => i < self.menu().len(),
            Focus::Grid => self.view == View::Library,
            Focus::Settings(_) => self.view == View::Settings,
            Focus::Home(..) => self.view == View::Home,
        };
        if !on_page {
            self.focus = Focus::Menu(self.current_menu());
        }
        // Sidebar, window padding, border and grid padding; each card is 150px plus a 20px gap.
        let chrome = if self.sidebar_open { 256. } else { 0. } + 16. + 4. + 2. + 48.;
        let content = window.viewport_size().width / px(1.) - chrome;
        self.columns = ((content + 20.) / 170.).floor().max(1.) as usize;
        let visible = self.visible(cx);
        self.visible_cache = visible.clone();
        let page = match self.view {
            View::Home => self.home_page(cx),
            View::Library => self.library_page(&visible, cx),
            View::Settings => self.settings_page(cx),
        };
        let mut body = div().flex_1().min_h_0().flex().p_2().gap_1();
        if self.sidebar_open {
            body = body.child(self.sidebar(cx));
        }
        body = body.child(
            div()
                .flex_1()
                .min_w_0()
                .h_full()
                .rounded_lg()
                .overflow_hidden()
                .border_1()
                .border_color(rgba(0xffffff14))
                .child(page),
        );
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(if self.appearance.glass { rgba(0x00000000) } else { self.appearance.fill(0x081012) })
            .font_family(FONT)
            .text_sm()
            .text_color(rgb(TEXT))
            .child(
                TitleBar::new().bg(self.appearance.fill(0x081012)).child(
                    div().flex_1().flex().items_center().gap_2().child(
                        div()
                            .id("toggle-sidebar")
                            .w(px(32.))
                            .h(px(32.))
                            .rounded_md()
                            .flex()
                            .items_center()
                            .justify_center()
                            .cursor_pointer()
                            .hover(|style| style.bg(rgba(0xffffff1a)))
                            .child(icon("sidebar", 18., rgb(if self.sidebar_open { TEXT } else { MUTED })))
                            .on_click(cx.listener(|view, _, _, cx| {
                                view.sidebar_open = !view.sidebar_open;
                                cx.notify();
                            })),
                    )
                    .child(div().font_weight(FontWeight::SEMIBOLD).child("EmuHub")),
                ),
            )
            .child(match &self.details {
                Some(d) => self.details_window(d, cx).into_any_element(),
                None => body.into_any_element(),
            })
            .child(self.bottom_bar())
    }
}

fn main() {
    Application::new().with_assets(assets::Assets).run(|cx: &mut App| {
        gpui_component::init(cx);
        Theme::change(ThemeMode::Dark, None, cx);
        Theme::global_mut(cx).colors.background = rgba(0x00000000).into();
        let bounds = Bounds::centered(None, size(px(1240.), px(800.)), cx);
        cx.open_window(
            WindowOptions {
                window_background: WindowBackgroundAppearance::Transparent,
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                window_min_size: Some(size(px(860.), px(560.))),
                titlebar: Some(TitlebarOptions {
                    title: Some("EmuHub".into()),
                    appears_transparent: true,
                    ..Default::default()
                }),
                ..Default::default()
            },
            |window, cx| {
                let view = cx.new(|cx| Hub::new(window, cx));
                cx.new(|cx| Root::new(view, window, cx))
            },
        )
        .expect("Could not open the EmuHub window");
        cx.on_window_closed(|cx| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();
        cx.activate(true);
    });
}
