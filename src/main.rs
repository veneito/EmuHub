#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod appearance;
mod assets;
mod emu;
mod gamepad;
mod update;

use emu::{Config, Game, System};
use gamepad::Pad;
use gpui::{prelude::*, *};
use gpui_component::{
    Disableable, Root, Selectable, Sizable, Theme, ThemeMode, TitleBar,
    button::{Button, ButtonVariants},
    input::{Input, InputEvent, InputState},
};

const MUTED: u32 = 0xa7b4ba;
const TEXT: u32 = 0xeeeae2;
const FONT: &str = if cfg!(target_os = "windows") { "Segoe UI" } else { ".SystemUIFont" };
/// Games the Ctrl+K search shows at once.
const SEARCH_RESULTS: usize = 8;

#[derive(Clone, Copy, PartialEq)]
enum View {
    Home,
    Library,
}

/// Where the controller is, RetroArch style: the side menu, the game grid, or the Settings popup.
#[derive(Clone, Copy, PartialEq)]
enum Focus {
    Menu(usize),
    Grid,
    /// A category in the Settings popup's list.
    SettingsCat(usize),
    /// A Settings button, by index into settings_items().
    Settings(usize),
    /// Home page: (row, column). Rows: 0 Continue Playing, 1 Favorites, 2 Systems.
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
    /// Install every missing core.
    Cores,
    /// Show the Cores category.
    CoreList,
    /// Install or reinstall CORES[i].
    Core(usize),
    /// Show or hide installed Steam games in PC Games.
    SteamGames,
    /// Add a folder to scan for PC games.
    AddPcFolder,
    /// Forget the added PC game folders.
    ClearPcFolders,
    /// Show every game hidden from its game page again.
    UnhideGames,
    /// Download (or refresh) LaunchBox's games database for art.
    LaunchBox,
    Palette(usize),
    SquareCovers(bool),
    Backdrop(appearance::Backdrop),
    /// Pick the program for emu::EMULATORS[i].
    PickEmulator(usize),
    /// Open emu::EMULATORS[i]'s official site.
    EmulatorSite(usize),
    /// Create and open Emulation/tools/<emulator>, to unzip it into.
    EmulatorFolder(usize),
    /// Install the newer EmuHub (or open the Releases page when this copy can't update itself).
    Update,
}

/// The full-screen game page (right-click or Options). `focus` walks Play, Favorite, Change cover, Back, then the cover choices.
struct Details {
    game: usize,
    /// None until "Change cover" is pressed; then the downloaded choices.
    candidates: Option<Vec<emu::Candidate>>,
    loading: bool,
    /// The Rename field is showing.
    renaming: bool,
    /// The emulator popup is open, with this choice focused.
    picking: Option<usize>,
    /// Which art the choices are for; LB/RB switch it.
    art: emu::Art,
    focus: usize,
    /// The cover choices scroll on their own, following the controller.
    scroll: ScrollHandle,
}

/// Play, Favorite, Change cover, Rename, Emulator, Program, Hide, Back.
const DETAIL_BUTTONS: usize = 8;
/// Cover choices per row: the 740px choices box fits six 110px tiles with 16px gaps.
const CHOICE_COLUMNS: usize = 6;

const PALETTES: [(&str, u32, u32); 6] = [
    ("Teal", 0x0c1417, 0x92c8e5),
    ("Midnight", 0x0b0b10, 0x9d7bff),
    ("Graphite", 0x151515, 0xc4c4ce),
    ("Blue", 0x101a2d, 0x92b9ff),
    ("Plum", 0x211324, 0xd5a8e3),
    ("Forest", 0x101f19, 0x91cba6),
];

/// A row of the sidebar's game list.
enum LibRow {
    /// A system's name and game count, starting its group.
    Header(String, usize),
    Game(usize),
}

/// The Settings popup's categories; Paths needs a keyboard, so it has no controller buttons.
const SETTING_CATEGORIES: [&str; 5] = ["Setup", "Cores", "Emulators", "Appearance", "Paths"];

/// emu::all_cores(), parsed once: settings_items() runs for every Settings button drawn.
static CORES: std::sync::LazyLock<Vec<String>> = std::sync::LazyLock::new(emu::all_cores);
/// The cores Install all gets: each system's own, plus the Zapper core.
static MAIN_CORES: std::sync::LazyLock<Vec<String>> = std::sync::LazyLock::new(emu::needed_cores);

/// (category, row in that category's card) of a Settings button.
fn setting_place(setting: Setting) -> (usize, usize) {
    match setting {
        Setting::EmulationFolder => (0, 0),
        Setting::RomFolders => (0, 1),
        Setting::RetroArch => (0, 2),
        Setting::CoreList | Setting::Cores => (0, 3),
        // Row 4 is BIOS files, which has no button.
        Setting::SteamGames => (0, 5),
        Setting::AddPcFolder | Setting::ClearPcFolders => (0, 6),
        Setting::UnhideGames => (0, 7),
        Setting::LaunchBox => (0, 8),
        Setting::Core(i) => (1, i),
        Setting::EmulatorFolder(i) | Setting::PickEmulator(i) | Setting::EmulatorSite(i) => (2, i),
        Setting::Palette(_) => (3, 0),
        Setting::Backdrop(_) => (3, 1),
        Setting::SquareCovers(_) => (3, 2),
        Setting::Update => (3, 3),
    }
}

/// The Settings buttons a controller can reach, in on-screen order: Setup, Cores, Emulators, Appearance.
fn settings_items() -> Vec<Setting> {
    let mut items = vec![Setting::EmulationFolder, Setting::RomFolders, Setting::RetroArch, Setting::CoreList, Setting::Cores, Setting::SteamGames, Setting::AddPcFolder, Setting::ClearPcFolders, Setting::UnhideGames, Setting::LaunchBox];
    items.extend((0..CORES.len()).map(Setting::Core));
    for i in 0..emu::EMULATORS.len() {
        items.extend([Setting::EmulatorFolder(i), Setting::PickEmulator(i), Setting::EmulatorSite(i)]);
    }
    items.extend((0..PALETTES.len()).map(Setting::Palette));
    items.extend([appearance::Backdrop::Solid, appearance::Backdrop::Color, appearance::Backdrop::Cover].map(Setting::Backdrop));
    items.extend([Setting::SquareCovers(false), Setting::SquareCovers(true)]);
    items.push(Setting::Update);
    items
}

/// The top-right update popup.
enum UpdateState {
    Hidden,
    Offer,
    Working(&'static str),
    Failed(String),
}

struct Hub {
    query: Entity<InputState>,
    /// The game page's Rename field.
    rename: Entity<InputState>,
    /// The game page's cover search field.
    cover_query: Entity<InputState>,
    /// Per-game settings from game pages, by scanned path.
    game_options: std::collections::HashMap<std::path::PathBuf, emu::GameOptions>,
    /// The Ctrl+K search popup is open, with this result picked; it types into `query`.
    search: Option<usize>,
    roms_dir: Entity<InputState>,
    retroarch_cmd: Entity<InputState>,
    cores_dir: Entity<InputState>,
    steamgriddb_key: Entity<InputState>,
    thegamesdb_key: Entity<InputState>,
    config: Config,
    systems: Vec<System>,
    games: Vec<Game>,
    view: View,
    /// The Settings popup is open, showing SETTING_CATEGORIES[settings_cat].
    settings_open: bool,
    settings_cat: usize,
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
    /// The sidebar's game list, also a uniform_list.
    side_scroll: UniformListScrollHandle,
    /// Games shown in the grid this frame, in order (filter + search applied).
    visible_cache: Vec<usize>,
    /// The sidebar list's rows this frame, with system headers.
    rows_cache: Vec<LibRow>,
    /// Systems whose group is open in the sidebar; they all start closed.
    expanded: std::collections::HashSet<String>,
    /// The library shows the selected game's page instead of the grid (a game was clicked).
    showing_game: bool,
    /// Games whose screenshot the Details view already asked libretro for.
    snaps_tried: std::collections::HashSet<std::path::PathBuf>,
    /// Each game's cover file if it has one, parallel to `games`, so drawing never touches the disk.
    covers: Vec<Option<std::path::PathBuf>>,
    /// Each game's icon, if one is cached; shown in the side list instead of a tiny cover.
    icons: Vec<Option<std::path::PathBuf>>,
    /// Each cover's color for the ambient background; None while it's being worked out or if it can't be.
    tints: std::collections::HashMap<std::path::PathBuf, Option<u32>>,
    /// Bumped on every rescan so an older cover download pass stops.
    cover_pass: usize,
    /// Bumped on every rescan so an older, slower scan doesn't replace a newer one.
    scan_pass: usize,
    /// The first scan has finished; until then pages show placeholders.
    scanned: bool,
    /// EmuHub's banner on disk, for the Home hero and background when there's no game art.
    banner: std::path::PathBuf,
    /// Picks the background's random favorite when no game is selected; set once per scan so it doesn't change on every redraw.
    idle_seed: usize,
    /// Horizontal scroll of the Home rows, so the controller's card stays in view.
    home_scroll: [ScrollHandle; 3],
    /// Vertical scroll of the Home page and the Settings popup's card, so the controller's item stays in view.
    home_page_scroll: ScrollHandle,
    settings_scroll: ScrollHandle,
    /// When each game was last played, for Continue Playing.
    history: std::collections::HashMap<std::path::PathBuf, u64>,
    /// Seconds played per game.
    playtime: std::collections::HashMap<std::path::PathBuf, u64>,
    /// Steam's own (play time, last played) for Steam games, by game path; merged into playtime and history.
    steam_stats: std::collections::HashMap<std::path::PathBuf, (u64, u64)>,
    favorites: std::collections::HashSet<std::path::PathBuf>,
    message: String,
    /// The message shown as a toast and when it appeared, so it can fade away on its own.
    toast: (String, std::time::Instant, usize),
    /// A newer release, once the startup check finds one, and the top-right popup's state.
    update: Option<update::Release>,
    update_state: UpdateState,
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
        // Keep full height so a long card scrolls instead of squeezing its rows.
        .flex_none()
        .items_center()
        .justify_between()
        .gap_6()
        .px_5()
        .py_3()
        .border_b_1()
        .border_color(rgba(0xffffff0f))
        .child(text)
}

/// The Library filter for favorite games, used in place of a system id.
const FAVORITES: &str = "favorites";

fn system_name<'a>(systems: &'a [System], id: &'a str) -> &'a str {
    if id == FAVORITES {
        return "Favorites";
    }
    systems.iter().find(|s| s.id == id).map_or(id, |s| s.name.as_str())
}

impl Hub {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let appearance = appearance::Appearance::load();
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
                // SteamGridDB keys are 32 characters; check once it's all there rather than on every keystroke.
                if view.config.steamgriddb_key.len() == 32 {
                    let key = view.config.steamgriddb_key.clone();
                    cx.spawn(async move |this, cx| {
                        let checked = cx
                            .background_executor()
                            .spawn(async move { emu::check_steamgriddb_key(&key).inspect(|_| emu::forget_cover_misses()) })
                            .await;
                        let _ = this.update(cx, |view, cx| {
                            match checked {
                                Ok(()) => {
                                    view.message = "SteamGridDB key works. Looking for missing covers.".into();
                                    view.fetch_covers(cx);
                                }
                                Err(error) => view.message = error,
                            }
                            cx.notify();
                        });
                    })
                    .detach();
                }
                cx.notify();
            }
        })
        .detach();
        let rename = cx.new(|cx| InputState::new(window, cx).placeholder("Leave empty for the file's name"));
        cx.subscribe_in(&rename, window, |view, _, event, window, cx| {
            if let InputEvent::PressEnter { .. } = event {
                view.save_rename(window, cx);
            }
        })
        .detach();
        let cover_query = cx.new(|cx| InputState::new(window, cx).placeholder("Search covers by name"));
        cx.subscribe_in(&cover_query, window, |view, input, event, _, cx| {
            if let InputEvent::PressEnter { .. } = event {
                let name = input.read(cx).value().trim().to_owned();
                view.search_covers(name, cx);
            }
        })
        .detach();
        let thegamesdb_key = cx.new(|cx| InputState::new(window, cx).masked(true).default_value(config.thegamesdb_key.clone()));
        // Saved as it's typed, like the SteamGridDB key.
        cx.subscribe_in(&thegamesdb_key, window, |view, input, event, _, cx| {
            if let InputEvent::Change = event {
                view.config.thegamesdb_key = input.read(cx).value().trim().to_owned();
                view.message = match view.config.save() {
                    Ok(()) if view.config.thegamesdb_key.is_empty() => "TheGamesDB key removed.".into(),
                    Ok(()) => "TheGamesDB key saved. Rescan to look for missing art.".into(),
                    Err(error) => format!("Could not save TheGamesDB key: {error}"),
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
            InputEvent::Change => {
                view.search = view.search.map(|_| 0);
                cx.notify();
            }
            _ => {}
        })
        .detach();
        // Ctrl+K (Cmd+K on Mac) anywhere opens the search, whatever has keyboard focus.
        let hub = cx.entity().downgrade();
        cx.intercept_keystrokes(move |event, window, cx| {
            let key = &event.keystroke;
            if key.key == "k" && key.modifiers.secondary() && !key.modifiers.shift && !key.modifiers.alt {
                let _ = hub.update(cx, |view, cx| {
                    if view.details.is_none() {
                        view.open_search(window, cx);
                    }
                });
                cx.stop_propagation();
            }
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
        // Notices when a game closes (to add its play time) and hides old toasts.
        cx.spawn(async move |this, cx| loop {
            cx.background_executor().timer(std::time::Duration::from_secs(1)).await;
            let tick = |view: &mut Self, cx: &mut Context<Self>| {
                if view.running.is_some() && !view.reap() {
                    cx.notify();
                }
                // Messages still in progress ("Downloading cores…") stay until they're replaced.
                if !view.message.is_empty() && !view.message.ends_with('…') && view.toast.1.elapsed().as_secs() >= 4 {
                    view.message.clear();
                    cx.notify();
                }
            };
            if this.update(cx, tick).is_err() {
                return;
            }
        })
        .detach();

        let mut hub = Self {
            query,
            rename,
            cover_query,
            game_options: emu::load_game_options(),
            search: None,
            roms_dir,
            retroarch_cmd,
            cores_dir,
            steamgriddb_key,
            thegamesdb_key,
            systems: emu::systems(),
            games: vec![],
            config,
            view: View::Home,
            settings_open: false,
            settings_cat: 0,
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
            side_scroll: UniformListScrollHandle::new(),
            visible_cache: vec![],
            rows_cache: vec![],
            expanded: Default::default(),
            showing_game: false,
            snaps_tried: Default::default(),
            covers: vec![],
            icons: vec![],
            tints: Default::default(),
            cover_pass: 0,
            scan_pass: 0,
            scanned: false,
            banner: emu::banner_path(),
            idle_seed: 0,
            home_scroll: [ScrollHandle::new(), ScrollHandle::new(), ScrollHandle::new()],
            home_page_scroll: ScrollHandle::new(),
            settings_scroll: ScrollHandle::new(),
            history: emu::load_history(),
            playtime: emu::load_playtime(),
            steam_stats: Default::default(),
            favorites: emu::load_favorites(),
            message: String::new(),
            toast: (String::new(), std::time::Instant::now(), 0),
            update: None,
            update_state: UpdateState::Hidden,
            appearance,
        };
        hub.rescan(cx);
        // Only asks GitHub whether there's a newer version; nothing downloads until Update is pressed.
        let skipped = hub.config.skipped_update.clone();
        cx.spawn(async move |this, cx| {
            let Some(release) = cx.background_executor().spawn(async move { update::check(skipped.as_deref()) }).await else { return };
            let _ = this.update(cx, |view, cx| {
                view.update = Some(release);
                view.update_state = UpdateState::Offer;
                cx.notify();
            });
            // The popup steps aside after a while; Settings keeps the Update button.
            cx.background_executor().timer(std::time::Duration::from_secs(20)).await;
            let _ = this.update(cx, |view, cx| {
                if matches!(view.update_state, UpdateState::Offer) {
                    view.update_state = UpdateState::Hidden;
                    cx.notify();
                }
            });
        })
        .detach();
        hub
    }

    /// Indexes into `games` that pass the system filter and search box.
    fn visible(&self, cx: &App) -> Vec<usize> {
        let query = self.query.read(cx).value().to_lowercase();
        (0..self.games.len())
            .filter(|&i| {
                let g = &self.games[i];
                let in_view = match self.system.as_deref() {
                    Some(FAVORITES) => self.favorites.contains(&g.path),
                    other => other.is_none_or(|s| s == g.system),
                };
                in_view && g.name.to_lowercase().contains(&query)
            })
            .collect()
    }

    /// The sidebar list: every system with games (matching the search), each followed by its games when open.
    /// Groups stay open while searching, so matches are never hidden.
    fn lib_rows(&self, cx: &App) -> Vec<LibRow> {
        let query = self.query.read(cx).value().to_lowercase();
        let mut rows = vec![];
        for system in &self.systems {
            let games: Vec<usize> = (0..self.games.len()).filter(|&i| self.games[i].system == system.id && self.games[i].name.to_lowercase().contains(&query)).collect();
            if games.is_empty() {
                continue;
            }
            rows.push(LibRow::Header(system.id.clone(), games.len()));
            if !query.is_empty() || self.expanded.contains(&system.id) {
                rows.extend(games.into_iter().map(LibRow::Game));
            }
        }
        rows
    }

    /// Header clicked: opens its group and shows the system's grid; clicking the open, shown system closes it.
    fn pick_system(&mut self, system: &str) {
        if self.view == View::Library && self.system.as_deref() == Some(system) && !self.showing_game && self.expanded.remove(system) {
            return;
        }
        self.expanded.insert(system.to_owned());
        self.show(&MenuItem::Library(Some(system.to_owned())));
    }

    /// A game clicked: the library shows its page.
    fn show_game(&mut self, index: usize) {
        if self.view != View::Library {
            self.view = View::Library;
            self.system = None;
        }
        self.selected = Some(index);
        self.showing_game = true;
        self.focus = Focus::Grid;
    }

    /// Scans on a background thread: big PC game folders can take seconds to walk.
    fn rescan(&mut self, cx: &mut Context<Self>) {
        self.scan_pass += 1;
        let pass = self.scan_pass;
        let (config, systems) = (self.config.clone(), self.systems.clone());
        cx.spawn(async move |this, cx| {
            let (games, covers, icons, steam) = cx
                .background_executor()
                .spawn(async move {
                    let mut games = emu::scan(&config.roms_dir, &systems);
                    games.extend(emu::pc_games(&config));
                    emu::apply_game_options(&mut games, &emu::load_game_options());
                    games.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
                    let covers = games.iter().map(|g| Some(emu::cover_path(g)).filter(|p| p.exists())).collect();
                    let icons = games.iter().map(|g| Some(emu::art_path(g, emu::Art::Icon)).filter(|p| p.exists())).collect();
                    let stats = emu::steam_play_stats();
                    let steam = games
                        .iter()
                        .filter_map(|g| Some((g.path.clone(), *stats.get(&emu::steam_appid(g)?)?)))
                        .collect();
                    (games, covers, icons, steam)
                })
                .await;
            let _ = this.update(cx, |view, cx| {
                if view.scan_pass == pass {
                    view.steam_stats = steam;
                    view.merge_steam_stats();
                    view.icons = icons;
                    view.set_games(games, covers, cx);
                }
            });
        })
        .detach();
    }

    fn set_games(&mut self, games: Vec<Game>, covers: Vec<Option<std::path::PathBuf>>, cx: &mut Context<Self>) {
        // Keep the selected game selected (and so its background) when it's still there.
        let selected = self.selected.map(|i| self.games[i].path.clone());
        let details = self.details.as_ref().map(|d| self.games[d.game].path.clone());
        self.scanned = true;
        self.games = games;
        self.covers = covers;
        // No rand crate for one pick: the clock's nanoseconds are random enough.
        self.idle_seed = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.subsec_nanos() as usize);
        self.selected = selected.and_then(|path| self.games.iter().position(|g| g.path == path));
        // The game page follows its game to its new place, and closes if the game was hidden.
        match details.and_then(|path| self.games.iter().position(|g| g.path == path)) {
            Some(i) => {
                if let Some(d) = &mut self.details {
                    d.game = i;
                }
            }
            None => self.details = None,
        }
        if self.system.as_deref().is_some_and(|s| s != FAVORITES && !self.games.iter().any(|g| g.system == s)) {
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
        self.icons = self.games.iter().map(|g| Some(emu::art_path(g, emu::Art::Icon)).filter(|p| p.exists())).collect();
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
        let tgdb = self.config.thegamesdb_key.clone();
        cx.spawn(async move |this, cx| {
            // 8 downloads at a time, redrawing once per batch.
            for batch in missing.chunks(8) {
                let tasks: Vec<_> = batch
                    .iter()
                    .cloned()
                    .map(|(index, game)| {
                        let (key, tgdb) = (key.clone(), tgdb.clone());
                        cx.background_executor().spawn(async move { (index, game.path.clone(), emu::fetch_cover(&game, &key, &tgdb).then(|| emu::cover_path(&game))) })
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
            // One-time cleanup of covers from older versions: full-size or not 2:3. Fixed ones are skipped after a header read.
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
        let open = self.reap();
        if open {
            self.message = "A game is already running. Close it first (Share + Options).".into();
        }
        open
    }

    /// True while the started emulator is open; once it closes, records its play time and forgets it.
    fn reap(&mut self) -> bool {
        let open = self.running.as_mut().is_some_and(|r| r.is_open());
        if let Some(running) = self.running.take_if(|_| !open) {
            running.record_playtime();
            self.playtime = emu::load_playtime();
            self.merge_steam_stats();
        }
        open
    }


    /// Steam games show Steam's play time and last played, when Steam's are the newer numbers.
    fn merge_steam_stats(&mut self) {
        for (path, &(secs, last)) in &self.steam_stats {
            let time = self.playtime.entry(path.clone()).or_default();
            *time = (*time).max(secs);
            let when = self.history.entry(path.clone()).or_default();
            *when = (*when).max(last);
        }
    }

    /// "Last played 2 days ago" and "3h 10m played", each on its own line so neither gets cut off.
    fn played_rows(&self, path: &std::path::Path) -> Div {
        let row = |name: &str, text: String| div().flex().items_center().gap_2().text_color(rgb(MUTED)).child(icon(name, 16., rgb(MUTED))).child(text);
        let last = self.history.get(path).map_or("Not played yet".into(), |&t| format!("Last played {}", emu::ago(t)));
        div()
            .flex()
            .flex_col()
            .gap_1()
            .child(row("history", last))
            .children(self.playtime.get(path).and_then(|&s| emu::playtime_text(s)).map(|time| row("play", time)))
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
        let game = game.clone();
        self.history = emu::load_history();
        self.merge_steam_stats();
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
        let mut items = vec![MenuItem::Home, MenuItem::Library(None), MenuItem::Library(Some(FAVORITES.into()))];
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
            _ if self.settings_open => MenuItem::Settings,
            View::Home => MenuItem::Home,
            View::Library => MenuItem::Library(self.system.clone()),
        };
        self.menu().iter().position(|m| *m == here).unwrap_or(0)
    }

    fn show(&mut self, item: &MenuItem) {
        match item {
            MenuItem::Home => {
                // The panel appears once a game is clicked on Home.
                if self.view != View::Home {
                    self.selected = None;
                }
                self.showing_game = false;
                self.view = View::Home;
            }
            MenuItem::Settings => self.settings_open = true,
            MenuItem::Library(system) => {
                if self.view != View::Library || self.system != *system {
                    self.selected = None;
                }
                self.showing_game = false;
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
            self.details_pad(action, window, cx);
            cx.notify();
            return;
        }
        if let Some(pick) = self.search {
            let results = self.search_results(cx);
            match action {
                Pad::Up => self.search = Some(pick.saturating_sub(1)),
                Pad::Down => self.search = Some((pick + 1).min(results.len().saturating_sub(1))),
                Pad::Confirm => self.search_pick(window, cx),
                Pad::Back => self.close_search(window, cx),
                _ => {}
            }
            cx.notify();
            return;
        }
        let menu = self.menu();
        match (self.focus, action) {
            (_, Pad::CloseGame) => {} // handled above
            (Focus::Settings(_) | Focus::SettingsCat(_), Pad::PrevTab | Pad::NextTab) => {} // not behind the Settings popup
            (_, Pad::PrevTab | Pad::NextTab) => {
                let tabs = menu.len() - 1; // Settings isn't a tab
                let at = self.current_menu().min(tabs - 1);
                let next = if action == Pad::NextTab { (at + 1) % tabs } else { (at + tabs - 1) % tabs };
                self.show(&menu[next]);
                self.focus = Focus::Menu(next);
            }
            // Side menu: up/down previews each page (but only X opens the Settings popup), X or Right goes into it.
            (Focus::Menu(i), Pad::Up | Pad::Down) => {
                let i = if action == Pad::Up { i.saturating_sub(1) } else { (i + 1).min(menu.len() - 1) };
                if menu[i] != MenuItem::Settings {
                    self.show(&menu[i]);
                }
                self.focus = Focus::Menu(i);
                // System entries are headers in the sidebar's list, which scrolls on its own.
                if let MenuItem::Library(Some(system)) = &menu[i] {
                    if let Some(row) = self.rows_cache.iter().position(|r| matches!(r, LibRow::Header(s, _) if s == system)) {
                        self.side_scroll.scroll_to_item(row, ScrollStrategy::Center);
                    }
                }
            }
            (Focus::Menu(i), Pad::Confirm | Pad::Right) => {
                match &menu[i] {
                    MenuItem::Library(Some(system)) if system != FAVORITES => {
                        self.expanded.insert(system.clone());
                    }
                    _ => {}
                }
                self.show(&menu[i]);
                if menu[i] == MenuItem::Settings {
                    self.focus = Focus::SettingsCat(self.settings_cat);
                } else if menu[i] == MenuItem::Home {
                    self.enter_home();
                } else if !self.visible(cx).is_empty() {
                    self.focus = Focus::Grid;
                    self.move_selection(0, cx);
                }
            }
            (Focus::Menu(_), Pad::Options) => self.open_search(window, cx),
            (Focus::Menu(_), _) => {}
            (Focus::Home(row, col), _) => self.home_pad(row, col, action, cx),
            // Game grid: Left from the first column, or O, returns to the menu.
            // A game's page: X plays, O goes back to the grid.
            (Focus::Grid, _) if self.showing_game => match action {
                Pad::Confirm => {
                    if let Some(i) = self.selected {
                        self.play(i, cx)
                    }
                }
                Pad::Options => {
                    if let Some(i) = self.selected {
                        self.open_details(i, cx)
                    }
                }
                Pad::Back => self.showing_game = false,
                Pad::Left => self.focus = Focus::Menu(self.current_menu()),
                _ => {}
            },
            (Focus::Grid, Pad::Back) => self.focus = Focus::Menu(self.current_menu()),
            (Focus::Grid, Pad::Left) if self.grid_position(cx).is_none_or(|p| p % self.columns == 0) => {
                self.focus = Focus::Menu(self.current_menu())
            }
            (Focus::Grid, Pad::Confirm) => match self.selected {
                Some(_) => self.showing_game = true,
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
            // Settings popup: up/down picks a category, X or Right goes into its buttons, O closes.
            (Focus::SettingsCat(_), Pad::Back | Pad::Left) => self.close_settings(),
            (Focus::SettingsCat(c), Pad::Up | Pad::Down) => {
                let c = if action == Pad::Up { c.saturating_sub(1) } else { (c + 1).min(SETTING_CATEGORIES.len() - 1) };
                self.settings_cat = c;
                self.focus = Focus::SettingsCat(c);
                self.settings_scroll.scroll_to_item(0);
            }
            (Focus::SettingsCat(c), Pad::Confirm | Pad::Right) => {
                if let Some(j) = settings_items().iter().position(|&s| setting_place(s).0 == c) {
                    self.focus = Focus::Settings(j);
                }
            }
            (Focus::SettingsCat(_), Pad::Options) => {}
            // Settings buttons: up/down within the category, X presses, O or Left back to the categories.
            (Focus::Settings(j), Pad::Back | Pad::Left) => self.focus = Focus::SettingsCat(setting_place(settings_items()[j]).0),
            (Focus::Settings(j), Pad::Up | Pad::Down) => {
                let items = settings_items();
                let cat = setting_place(items[j]).0;
                let next = if action == Pad::Up {
                    (0..j).rev().find(|&k| setting_place(items[k]).0 == cat)
                } else {
                    (j + 1..items.len()).find(|&k| setting_place(items[k]).0 == cat)
                };
                if let Some(k) = next {
                    self.focus = Focus::Settings(k);
                    self.settings_scroll.scroll_to_item(setting_place(items[k]).1);
                }
            }
            (Focus::Settings(j), Pad::Confirm) => self.apply_setting(settings_items()[j], window, cx),
            (Focus::Settings(_), Pad::Right | Pad::Options) => {}
        }
        cx.notify();
    }

    fn open_details(&mut self, game: usize, cx: &mut Context<Self>) {
        self.selected = Some(game);
        self.details = Some(Details { game, candidates: None, loading: false, renaming: false, picking: None, art: emu::Art::Cover, focus: 0, scroll: ScrollHandle::new() });
        cx.notify();
    }

    /// Controller on the game page: any direction steps through the buttons and cover choices.
    fn details_pad(&mut self, action: Pad, window: &mut Window, cx: &mut Context<Self>) {
        let Some(details) = &mut self.details else { return };
        let count = DETAIL_BUTTONS + details.candidates.as_ref().map_or(0, Vec::len);
        let focus = details.focus;
        if let Some(pick) = details.picking {
            let game = details.game;
            let count = self.emulator_choices(&self.games[game]).len();
            let Some(details) = &mut self.details else { return };
            match action {
                Pad::Up => details.picking = Some(pick.saturating_sub(1)),
                Pad::Down => details.picking = Some((pick + 1).min(count.saturating_sub(1))),
                Pad::Confirm => self.choose_emulator(pick, cx),
                Pad::Back | Pad::Options => details.picking = None,
                _ => {}
            }
            return;
        }
        // Buttons are one row; covers follow in rows of CHOICE_COLUMNS.
        let next = match action {
            Pad::Back if details.renaming => {
                details.renaming = false;
                return;
            }
            Pad::Back | Pad::Options => {
                self.details = None;
                return;
            }
            Pad::Confirm => return self.details_action(focus, window, cx),
            Pad::PrevTab | Pad::NextTab if details.candidates.is_some() || details.loading => {
                let at = emu::Art::ALL.iter().position(|&a| a == details.art).unwrap_or(0);
                let next = if action == Pad::NextTab { (at + 1) % emu::Art::ALL.len() } else { (at + emu::Art::ALL.len() - 1) % emu::Art::ALL.len() };
                return self.switch_art(emu::Art::ALL[next], cx);
            }
            Pad::PrevTab | Pad::NextTab | Pad::CloseGame => return,
            Pad::Left => focus.saturating_sub(1),
            Pad::Right => (focus + 1).min(count - 1),
            // Back sits above the button row.
            Pad::Up if focus < DETAIL_BUTTONS => DETAIL_BUTTONS - 1,
            Pad::Down if focus == DETAIL_BUTTONS - 1 => 0,
            Pad::Up if focus < DETAIL_BUTTONS + CHOICE_COLUMNS => 2, // first row of covers → Change cover
            Pad::Up => focus - CHOICE_COLUMNS,
            Pad::Down if focus < DETAIL_BUTTONS => DETAIL_BUTTONS.min(count - 1),
            Pad::Down => (focus + CHOICE_COLUMNS).min(count - 1),
        };
        details.focus = next;
        if next >= DETAIL_BUTTONS {
            details.scroll.scroll_to_item(next - DETAIL_BUTTONS);
        }
    }

    /// Changes one game's settings, saves them, and rescans when the library shows the change.
    fn set_game_option(&mut self, path: std::path::PathBuf, change: impl FnOnce(&mut emu::GameOptions), cx: &mut Context<Self>) {
        let options = self.game_options.entry(path).or_default();
        let before = (options.name.clone(), options.hidden);
        change(options);
        let rescan = before != (options.name.clone(), options.hidden);
        if let Err(error) = emu::save_game_options(&self.game_options) {
            self.message = format!("Could not save game settings: {error}");
        }
        if rescan {
            self.rescan(cx);
        }
    }

    fn save_rename(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(details) = &mut self.details else { return };
        details.renaming = false;
        let path = self.games[details.game].path.clone();
        let name = self.rename.read(cx).value().trim().to_owned();
        self.set_game_option(path, |o| o.name = (!name.is_empty()).then_some(name), cx);
        window.blur();
        cx.notify();
    }

    /// The cores and emulators a game can use: its system's own first, then the alternates.
    fn emulator_choices(&self, game: &Game) -> Vec<String> {
        let Some(system) = self.systems.iter().find(|s| s.id == game.system) else { return Vec::new() };
        std::iter::once(system.default_emulator().to_owned()).chain(system.alts.iter().cloned()).filter(|c| !c.is_empty()).collect()
    }

    /// Looks up cover choices for the game page's game under `name` (its own name, or one typed in the search field).
    fn search_covers(&mut self, name: String, cx: &mut Context<Self>) {
        let Some(details) = &mut self.details else { return };
        if details.loading || name.is_empty() {
            return;
        }
        details.loading = true;
        details.candidates = None;
        let mut game = self.games[details.game].clone();
        game.name = name;
        let (key, tgdb, kind) = (self.config.steamgriddb_key.clone(), self.config.thegamesdb_key.clone(), details.art);
        cx.spawn(async move |this, cx| {
            let found = cx.background_executor().spawn(async move { emu::art_candidates(&game, kind, &key, &tgdb) }).await;
            let _ = this.update(cx, |view, cx| {
                if let Some(details) = &mut view.details {
                    details.loading = false;
                    details.candidates = Some(found);
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    /// Shows the choices for another kind of art (cover, background, logo, icon), searching the same name.
    fn switch_art(&mut self, kind: emu::Art, cx: &mut Context<Self>) {
        let Some(details) = &mut self.details else { return };
        if details.loading || details.art == kind {
            return;
        }
        details.art = kind;
        details.focus = 2;
        let name = self.cover_query.read(cx).value().trim().to_owned();
        self.search_covers(name, cx);
    }

    /// Asks for an image file and makes it the game page's game's art of the kind being chosen.
    fn cover_from_file(&mut self, cx: &mut Context<Self>) {
        let Some(details) = &self.details else { return };
        let (game, kind) = (self.games[details.game].clone(), details.art);
        let picked = cx.prompt_for_paths(PathPromptOptions { files: true, directories: false, multiple: false, prompt: Some(format!("Choose a {} image", kind.label().to_lowercase()).into()) });
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(Some(mut paths))) = picked.await {
                let file = paths.remove(0);
                let _ = this.update(cx, |view, cx| {
                    view.run("Saving art…", move |_| emu::set_art_file(&game, kind, &file).map(|()| format!("{} updated.", kind.label())), cx)
                });
            }
        })
        .detach();
    }

    /// Sets the game page's game to run with its n-th emulator choice, and closes the popup.
    fn choose_emulator(&mut self, n: usize, cx: &mut Context<Self>) {
        let Some(details) = &mut self.details else { return };
        details.picking = None;
        let game = self.games[details.game].clone();
        let choices = self.emulator_choices(&game);
        let Some(choice) = choices.get(n).cloned() else { return };
        self.message = format!("{} now uses {}.", game.name, emu::emulator(&choice).map_or(choice.as_str(), |e| e.name));
        // Back at the system's own choice, nothing needs saving.
        let emulator = (n != 0).then_some(choice);
        self.set_game_option(game.path, |o| o.emulator = emulator, cx);
        cx.notify();
    }

    /// The emulator popup over the game page: one row per choice, the current one marked.
    fn emulator_popup(&self, details: &Details, pick: usize, cx: &mut Context<Self>) -> Div {
        let game = &self.games[details.game];
        let current = self.game_options.get(&game.path).and_then(|o| o.emulator.clone());
        let mut list = card();
        for (n, choice) in self.emulator_choices(game).into_iter().enumerate() {
            let (name, installed) = match emu::emulator(&choice) {
                Some(e) => (e.name.to_owned(), emu::find_emulator(&self.config, e).is_some()),
                None => (choice.clone(), emu::core_installed(&self.config, &choice)),
            };
            let mut note = vec![if n == 0 { "System default" } else { "Alternate" }];
            note.push(if installed { "installed" } else { "not installed: Settings → Cores or Emulators" });
            let chosen = current.as_deref().map_or(n == 0, |c| c == choice);
            list = list.child(
                self.ring(setting_row(&name, &note.join(", ")).rounded_md(), pick == n)
                    .id(("emulator-choice", n))
                    .cursor_pointer()
                    .hover(|style| style.bg(rgba(0xffffff0a)))
                    .on_click(cx.listener(move |view, _, _, cx| view.choose_emulator(n, cx)))
                    .child(if chosen { icon("play", 16., rgb(self.appearance.accent)) } else { icon("none", 16., rgb(MUTED)) }),
            );
        }
        div()
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .bg(rgba(0x00000099))
            .occlude()
            .child(
                div()
                    .w(px(520.))
                    .p_6()
                    .flex()
                    .flex_col()
                    .gap_4()
                    .rounded_lg()
                    .bg(self.appearance.fill(0x101e23))
                    .border_1()
                    .border_color(rgba(0xffffff14))
                    .child(div().text_size(px(22.)).font_weight(FontWeight::SEMIBOLD).child(format!("Run {} with", game.name)))
                    .child(list),
            )
    }

    /// What a game runs with, as shown on its page.
    fn emulator_label(&self, game: &Game) -> String {
        let choice = self.game_options.get(&game.path).and_then(|o| o.emulator.clone());
        let id = choice.or_else(|| self.emulator_choices(game).into_iter().next()).unwrap_or_default();
        emu::emulator(&id).map_or(id.clone(), |e| e.name.to_owned())
    }

    /// 0 = Play, 1 = Favorite, 2 = Change cover, 3 = Rename, 4 = Emulator, 5 = Program, 6 = Hide, 7 = Back,
    /// 8.. = pick that cover.
    fn details_action(&mut self, item: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(details) = &mut self.details else { return };
        let game = self.games[details.game].clone();
        let pc_program = game.system == emu::PC && game.path.extension().is_none_or(|e| e != "acf");
        match item {
            0 => {
                let index = details.game;
                self.details = None;
                self.play(index, cx);
            }
            1 => {
                let index = details.game;
                self.toggle_favorite(index);
            }
            2 if !details.loading => {
                let name = game.name.clone();
                self.cover_query.update(cx, |input, cx| input.set_value(name.clone(), window, cx));
                self.search_covers(name, cx);
            }
            2 => {}
            3 if details.renaming => self.save_rename(window, cx),
            3 => {
                details.renaming = true;
                let name = game.name.clone();
                self.rename.update(cx, |input, cx| {
                    input.set_value(name, window, cx);
                    input.focus(window, cx);
                });
            }
            4 => {
                let choices = self.emulator_choices(&game);
                if choices.len() > 1 {
                    let current = self.game_options.get(&game.path).and_then(|o| o.emulator.clone()).unwrap_or_else(|| choices[0].clone());
                    let pick = choices.iter().position(|c| *c == current).unwrap_or(0);
                    if let Some(details) = &mut self.details {
                        details.picking = Some(pick);
                    }
                }
            }
            5 if pc_program => {
                let picked = cx.prompt_for_paths(PathPromptOptions {
                    files: true,
                    directories: false,
                    multiple: false,
                    prompt: Some(format!("Choose the program for {}", game.name).into()),
                });
                cx.spawn(async move |this, cx| {
                    if let Ok(Ok(Some(mut paths))) = picked.await {
                        let _ = this.update(cx, |view, cx| {
                            let program = paths.remove(0);
                            view.message = format!("{} now starts {}.", game.name, program.display());
                            view.set_game_option(game.path.clone(), |o| o.program = Some(program), cx);
                            cx.notify();
                        });
                    }
                })
                .detach();
            }
            5 => {}
            6 => {
                self.message = format!("{} hidden. Settings → Setup → Hidden games brings it back.", game.name);
                self.set_game_option(game.path.clone(), |o| o.hidden = true, cx);
            }
            7 => self.details = None,
            n => {
                let Some(choice) = details.candidates.as_ref().and_then(|c| c.get(n - DETAIL_BUTTONS)) else { return };
                let (url, kind) = (choice.url.clone(), details.art);
                self.run("Downloading art…", move |_| emu::set_art(&game, kind, &url).map(|()| format!("{} updated.", kind.label())), cx);
            }
        }
        cx.notify();
    }

    fn details_window(&self, details: &Details, cx: &mut Context<Self>) -> Stateful<Div> {
        let game = &self.games[details.game];
        let cover = emu::cover_path(game);
        let art = if cover.exists() {
            img(cover).size_full().rounded(self.cover_corners()).object_fit(ObjectFit::Cover).into_any_element()
        } else {
            tile(&game.name, 160.).size_full().rounded(self.cover_corners()).into_any_element()
        };
        let focused = |i: usize| details.focus == i;
        // Round icon buttons with a label underneath; Play is filled with the accent color.
        let mut buttons = div().flex().gap_6().pt_4();
        let favorite = self.favorites.contains(&game.path);
        let (heart, heart_label) = if favorite { ("heart-filled", "Unfavorite") } else { ("heart", "Favorite") };
        let emulator = self.emulator_label(game);
        let pc_program = game.system == emu::PC && game.path.extension().is_none_or(|e| e != "acf");
        let has_choices = self.emulator_choices(game).len() > 1;
        for (i, (id, name, label)) in [
            ("detail-play", "play", "Play"),
            ("detail-favorite", heart, heart_label),
            ("detail-cover", "image", "Change art"),
            ("detail-rename", "edit", if details.renaming { "Save name" } else { "Rename" }),
            ("detail-emulator", "gamepad", emulator.as_str()),
            ("detail-program", "folder", "Program"),
            ("detail-hide", "eye-off", "Hide"),
        ]
        .into_iter()
        .enumerate()
        {
            // Emulator only for games with a choice, Program only for PC programs.
            let unavailable = (i == 4 && !has_choices) || (i == 5 && !pc_program);
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
                    .when((i == 2 && details.loading) || unavailable, |d| d.opacity(0.4))
                    .hover(|style| style.opacity(0.85))
                    .on_click(cx.listener(move |view, _, window, cx| view.details_action(i, window, cx)))
                    .child(circle)
                    .child(div().text_xs().text_color(rgb(MUTED)).child(label.to_owned())),
            );
        }

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
                    .child(div().w(px(240.)).h(px(360.)).flex_none().rounded(self.cover_corners()).overflow_hidden().shadow_lg().child(art))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .h(px(360.))
                            .flex()
                            .flex_col()
                            .gap_2()
                            // Back in the top corner, level with the cover's top edge.
                            .child(
                                div()
                                    .id("detail-close")
                                    .flex_1()
                                    .child(
                                        self.ring(
                                            div().w(px(44.)).h(px(44.)).rounded_full().flex().items_center().justify_center().cursor_pointer().bg(rgba(0xffffff1a)),
                                            focused(DETAIL_BUTTONS - 1),
                                        )
                                        .id("detail-back")
                                        .hover(|style| style.opacity(0.85))
                                        .on_click(cx.listener(|view, _, window, cx| view.details_action(DETAIL_BUTTONS - 1, window, cx)))
                                        .child(icon("arrow-left", 20., rgb(TEXT))),
                                    ),
                            )
                            .child(heading(system_name(&self.systems, &game.system)))
                            .child(div().text_size(px(40.)).font_weight(FontWeight::SEMIBOLD).child(game.name.clone()))
                            .child(self.played_rows(&game.path))
                            .child(div().text_xs().text_color(rgb(MUTED)).child(game.path.display().to_string()))
                            .child(buttons),
                    )
                    // The logo on the right, in the layout rather than over it, so a narrow window shrinks it
                    // and wraps the title instead of overlapping them.
                    .children(Some(emu::art_path(game, emu::Art::Logo)).filter(|l| l.exists()).map(|logo| {
                        // As tall as the cover, with the logo in its top half, so it lines up with the cover's top.
                        div()
                            .w(px(360.))
                            .h(px(360.))
                            .flex_shrink()
                            .min_w_0()
                            .child(div().w_full().h(px(180.)).child(img(logo).size_full().object_fit(ObjectFit::Contain)))
                    })),
            );
        if details.renaming {
            panel = panel.child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(div().w(px(420.)).child(Input::new(&self.rename)))
                    .child(Button::new("rename-save").primary().label("Save").on_click(cx.listener(|view, _, window, cx| view.save_rename(window, cx)))),
            );
        }

        if details.loading || details.candidates.is_some() {
            // Another name to search for, or an image of your own.
            panel = panel.child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(div().w(px(420.)).child(Input::new(&self.cover_query)))
                    .child(Button::new("cover-search").label("Search").disabled(details.loading).on_click(cx.listener(|view, _, _, cx| {
                        let name = view.cover_query.read(cx).value().trim().to_owned();
                        view.search_covers(name, cx);
                    })))
                    .child(Button::new("cover-file").label("From file…").on_click(cx.listener(|view, _, _, cx| view.cover_from_file(cx)))),
            );
            // Which art to choose; LB/RB on a controller.
            let mut tabs = div().flex().gap_2();
            for kind in emu::Art::ALL {
                tabs = tabs.child(
                    Button::new(SharedString::from(format!("art-{}", kind.label())))
                        .label(kind.label())
                        .selected(details.art == kind)
                        .on_click(cx.listener(move |view, _, _, cx| view.switch_art(kind, cx))),
                );
            }
            panel = panel.child(tabs);
        }
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
                        .on_click(cx.listener(move |view, _, window, cx| view.details_action(item, window, cx)))
                        .child(
                            self.ring(div().w(px(110.)).h(px(165.)).rounded_md().overflow_hidden(), focused(item))
                                // Covers fill the box; wide backgrounds, logos and icons are shown whole on a dark backing.
                                .when(details.art != emu::Art::Cover, |d| d.bg(rgba(0x00000060)).p_1())
                                .child(img(choice.preview.clone()).size_full().object_fit(if details.art == emu::Art::Cover { ObjectFit::Cover } else { ObjectFit::Contain })),
                        )
                        .child(div().text_xs().text_color(rgb(MUTED)).line_clamp(2).child(choice.label.clone())),
                );
            }
            panel = panel.child(heading(&format!("Choose a {}", details.art.label().to_lowercase()))).child(if candidates.is_empty() {
                div().text_color(rgb(MUTED)).child("Nothing found. Try another name, or add SteamGridDB and TheGamesDB keys in Settings → Paths.").into_any_element()
            } else {
                choices.into_any_element()
            });
        }

        // Full page between the title bar and the hints bar, over a dimmed background (or the cover).
        let hero = emu::art_path(game, emu::Art::Hero);
        let (cover, opacity) = if hero.exists() { (hero, 0.3) } else { (emu::cover_path(game), 0.12) };
        div()
            .id("details")
            .relative()
            .flex_1()
            .min_h_0()
            .overflow_hidden()
            .bg(self.appearance.fill(0x101e23))
            .when(self.appearance.backdrop == appearance::Backdrop::Cover && cover.exists(), |d| {
                d.child(div().absolute().top_0().left_0().size_full().opacity(opacity).child(img(cover).size_full().object_fit(ObjectFit::Cover)))
            })
            .child(panel)
            .when_some(details.picking, |d, pick| d.child(self.emulator_popup(details, pick, cx)))
    }

    /// Position of the selected game within the visible list.
    fn grid_position(&self, cx: &App) -> Option<usize> {
        let selected = self.selected?;
        self.visible(cx).iter().position(|&v| v == selected)
    }

    fn close_settings(&mut self) {
        self.settings_open = false;
        self.focus = Focus::Menu(self.current_menu());
    }

    fn toggle_favorite(&mut self, index: usize) {
        let path = &self.games[index].path;
        if !self.favorites.remove(path) {
            self.favorites.insert(path.clone());
        }
        if let Err(error) = emu::save_favorites(&self.favorites) {
            self.message = format!("Could not save favorites: {error}");
        }
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
            Setting::RetroArch if emu::retroarch_version_warning(&self.config).is_some() => {
                self.message = emu::open_url(emu::RETROARCH_INTEL_MAC_URL).err().unwrap_or_default();
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
            Setting::SteamGames => {
                self.config.hide_steam = !self.config.hide_steam;
                self.message = self.config.save().err().unwrap_or_default();
                self.rescan(cx);
            }
            Setting::AddPcFolder => {
                let picked = cx.prompt_for_paths(PathPromptOptions {
                    files: false,
                    directories: true,
                    multiple: false,
                    prompt: Some("Choose a folder of PC games".into()),
                });
                cx.spawn(async move |this, cx| {
                    if let Ok(Ok(Some(mut paths))) = picked.await {
                        let _ = this.update(cx, |view, cx| {
                            let folder = paths.remove(0);
                            if !view.config.pc_folders.contains(&folder) {
                                view.config.pc_folders.push(folder);
                            }
                            view.message = view.config.save().err().unwrap_or_default();
                            view.rescan(cx);
                            cx.notify();
                        });
                    }
                })
                .detach();
            }
            Setting::LaunchBox if !self.busy => {
                self.busy = true;
                self.message = "Downloading the LaunchBox database (about 110 MB)…".into();
                cx.spawn(async move |this, cx| {
                    let result = cx
                        .background_executor()
                        .spawn(async move {
                            let count = emu::update_launchbox()?;
                            // Games that had no art may have some now.
                            emu::forget_art_misses();
                            Ok::<_, String>(count)
                        })
                        .await;
                    let _ = this.update(cx, |view, cx| {
                        view.busy = false;
                        view.message = match result {
                            Ok(count) => format!("LaunchBox database ready: art for {count} games. Looking for missing covers."),
                            Err(error) => error,
                        };
                        view.fetch_covers(cx);
                        cx.notify();
                    });
                })
                .detach();
            }
            Setting::LaunchBox => {}
            Setting::UnhideGames => {
                for options in self.game_options.values_mut() {
                    options.hidden = false;
                }
                self.message = emu::save_game_options(&self.game_options).err().unwrap_or_default();
                self.rescan(cx);
            }
            Setting::ClearPcFolders => {
                self.config.pc_folders.clear();
                self.message = self.config.save().err().unwrap_or_default();
                self.rescan(cx);
            }
            Setting::CoreList => {
                self.settings_cat = 1;
                self.settings_scroll.scroll_to_item(0);
                if let Some(j) = settings_items().iter().position(|&s| setting_place(s).0 == 1) {
                    self.focus = Focus::Settings(j);
                }
            }
            Setting::Core(i) => {
                let core = CORES[i].clone();
                self.run(&format!("Downloading {core}…"), move |config| emu::install_core(&config, &core).map(|()| format!("Installed {core}.")), cx)
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
            Setting::Update => self.start_update(cx),
            Setting::EmulatorSite(i) => self.message = emu::open_url(emu::EMULATORS[i].site).err().unwrap_or_default(),
            Setting::Palette(i) => {
                let (_, surface, accent) = PALETTES[i];
                self.appearance.surface = surface;
                self.appearance.accent = accent;
                self.save_appearance(cx);
            }
            Setting::Backdrop(backdrop) => {
                self.appearance.backdrop = backdrop;
                self.save_appearance(cx);
            }
            Setting::SquareCovers(square) => {
                self.appearance.square_covers = square;
                self.save_appearance(cx);
            }
        }
        cx.notify();
    }

    /// Accent outline around whatever the controller is on.
    /// Corner radius for game covers, from Settings → Appearance.
    fn cover_corners(&self) -> Pixels {
        px(if self.appearance.square_covers { 0. } else { 8. })
    }

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
    /// Update pressed: downloads and checks the new version, installs it and quits so it can restart.
    /// A copy that can't update itself (portable exe, bare Linux binary) opens the Releases page instead.
    fn start_update(&mut self, cx: &mut Context<Self>) {
        let release = match self.update.clone() {
            Some(release) if release.installs_itself() => release,
            _ => {
                self.message = emu::open_url(update::RELEASES_URL).err().unwrap_or_default();
                return;
            }
        };
        if matches!(self.update_state, UpdateState::Working(_)) || self.game_running() {
            return;
        }
        self.update_state = UpdateState::Working("Downloading and checking the update…");
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { update::download_verified(&release).and_then(|file| update::install(&file, &update::Install::detect())) })
                .await;
            let _ = this.update(cx, |view, cx| match result {
                Ok(()) => cx.quit(),
                Err(error) => {
                    view.update_state = UpdateState::Failed(error);
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// Skip: this version isn't offered again; a newer one will be.
    fn skip_update(&mut self, cx: &mut Context<Self>) {
        if let Some(release) = self.update.take() {
            self.config.skipped_update = Some(release.version);
            let _ = self.config.save();
        }
        self.update_state = UpdateState::Hidden;
        cx.notify();
    }

    /// Top-right card: "EmuHub X is available" with Skip / Update, then progress or the error.
    fn update_popup(&self, cx: &mut Context<Self>) -> Option<Div> {
        let release = self.update.as_ref()?;
        let failed = matches!(self.update_state, UpdateState::Failed(_));
        let (title, note) = match &self.update_state {
            UpdateState::Hidden => return None,
            UpdateState::Offer => (format!("EmuHub {} is available", release.version), "With a controller: Settings → Update.".to_owned()),
            UpdateState::Working(step) => (format!("Updating to {}", release.version), step.to_string()),
            UpdateState::Failed(error) => ("Update failed".to_owned(), error.clone()),
        };
        let mut buttons = div().flex().justify_end().gap_2();
        if !matches!(self.update_state, UpdateState::Working(_)) {
            let main_label = if failed || !release.installs_itself() { "Open releases" } else { "Update" };
            buttons = buttons
                .child(Button::new("popup-skip").label(if failed { "Close" } else { "Skip" }).on_click(cx.listener(move |view, _, _, cx| {
                    if failed {
                        view.update_state = UpdateState::Hidden;
                        cx.notify();
                    } else {
                        view.skip_update(cx);
                    }
                })))
                .child(Button::new("popup-update").primary().label(main_label).on_click(cx.listener(move |view, _, _, cx| {
                    if failed {
                        view.message = emu::open_url(update::RELEASES_URL).err().unwrap_or_default();
                    } else {
                        view.start_update(cx);
                    }
                })));
        }
        Some(
            div()
                .absolute()
                .top(px(48.))
                .right(px(16.))
                .w(px(320.))
                .p_4()
                .flex()
                .flex_col()
                .gap_2()
                .rounded_lg()
                .border_1()
                .border_color(rgb(self.appearance.accent))
                .bg(self.appearance.fill(0x101e23))
                .shadow_lg()
                .occlude()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(icon("refresh", 16., rgb(self.appearance.accent)))
                        .child(div().font_weight(FontWeight::SEMIBOLD).child(title)),
                )
                .child(div().text_xs().text_color(rgb(MUTED)).child(note))
                .child(buttons),
        )
    }

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
        self.config.thegamesdb_key = read(&self.thegamesdb_key);
        // A new key may find art that was missing before.
        cx.background_executor().spawn(async { emu::forget_art_misses() }).detach();
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
            .h(px(36.))
            .flex_none()
            .rounded_md()
            .cursor_pointer()
            .text_size(px(14.))
            .text_color(rgb(if selected { 0xf5f3ee } else { 0xc3cdd1 }))
            .bg(if selected { rgba((self.appearance.accent << 8) | 0x30) } else { rgba(0x00000000) })
            .hover(|style| style.bg(rgba(0xffffff12)))
    }

    fn sidebar(&self, cx: &mut Context<Self>) -> Stateful<Div> {
        let tone = |selected: bool| rgb(if selected { 0xf5f3ee } else { MUTED });
        let all = self.view == View::Library && self.system.is_none();
        let settings = self.settings_open;
        let menu_len = self.menu().len();
        let focused = |i: usize| self.focus == Focus::Menu(i);
        let home = self.view == View::Home;
        let shortcut = if cfg!(target_os = "macos") { "⌘K" } else { "Ctrl K" };
        let nav = div()
            .flex()
            .flex_col()
            .gap_1()
            // Looks like a search field; opens the Ctrl+K search (Options on the controller).
            .child(
                div()
                    .id("nav-search")
                    .flex()
                    .items_center()
                    .gap_2()
                    .px_3()
                    .h(px(36.))
                    .mb_2()
                    .rounded_md()
                    .cursor_pointer()
                    .bg(rgba(0xffffff0a))
                    .border_1()
                    .border_color(rgba(0xffffff14))
                    .hover(|style| style.bg(rgba(0xffffff12)))
                    .text_color(rgb(MUTED))
                    .child(icon("search", 16., rgb(MUTED)))
                    .child(div().flex_1().child("Search games"))
                    .child(div().px_1p5().rounded_sm().border_1().border_color(rgba(0xffffff24)).text_xs().child(shortcut))
                    .on_click(cx.listener(|view, _, window, cx| view.open_search(window, cx))),
            )
            .child(div().px_3().pb_1().child(heading("Browse")))
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
            .child({
                let favorites = self.view == View::Library && self.system.as_deref() == Some(FAVORITES);
                self.ring(self.nav_row("nav-favorites", favorites), focused(2))
                    .child(icon(if favorites { "heart-filled" } else { "heart" }, 18., tone(favorites)))
                    .child(div().flex_1().child("Favorites"))
                    .child(div().text_xs().text_color(rgb(MUTED)).child(self.favorites.len().to_string()))
                    .on_click(cx.listener(|view, _, _, cx| {
                        view.view = View::Library;
                        view.system = Some(FAVORITES.into());
                        cx.notify();
                    }))
            })
            // Divides the pages from the systems below.
            .child(div().mx_3().my_2().h(px(1.)).bg(rgba(0xffffff1a)));
        // The game list scrolls on its own; the sidebar around it doesn't.
        let nav = nav.flex_1().min_h_0().child(self.side_list(cx));
        div()
            .id("sidebar")
            .w(px(320.))
            .h_full()
            .flex_shrink_0()
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
                        view.settings_open = true;
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
        // Playnite-style toolbar: title and count, then the library buttons.
        let toolbar = div()
            .flex()
            .items_center()
            .gap_3()
            .px_6()
            .pt_4()
            .pb_3()
            // Solid bar with a line under it; the page below is see-through to the window's background.
            .bg(self.appearance.fill(0x0c1417))
            .border_b_1()
            .border_color(rgba(0xffffff14))
            // On a game's page: back to the grid it came from.
            .when(self.showing_game, |bar| {
                bar.child(
                    div()
                        .id("game-back")
                        .w(px(32.))
                        .h(px(32.))
                        .rounded_md()
                        .flex()
                        .items_center()
                        .justify_center()
                        .cursor_pointer()
                        .bg(rgba(0xffffff0a))
                        .hover(|style| style.bg(rgba(0xffffff1a)))
                        .child(icon("arrow-left", 18., rgb(TEXT)))
                        .on_click(cx.listener(|view, _, _, cx| {
                            view.showing_game = false;
                            cx.notify();
                        })),
                )
            })
            .child(div().text_size(px(22.)).font_weight(FontWeight::SEMIBOLD).child(title))
            .child(div().text_color(rgb(MUTED)).child(format!("{} game{}", visible.len(), if visible.len() == 1 { "" } else { "s" })))
            .child(div().flex_1())
            .child(Button::new("rescan").small().label("Rescan").on_click(cx.listener(|view, _, _, cx| view.rescan(cx))))
            .child(Button::new("open-library").small().label("Open").on_click(cx.listener(|view, _, _, cx| {
                // The system's ROM folder, or the whole ROM folder for All games and Favorites.
                let dir = match view.system.as_deref() {
                    Some(id) if id != FAVORITES => view.config.roms_dir.join(id),
                    _ => view.config.roms_dir.clone(),
                };
                if let Err(error) = emu::open_folder(&dir) {
                    view.message = error;
                    cx.notify();
                }
            })));
        let page = div().id("library").size_full().flex().flex_col().child(toolbar);
        if self.showing_game && self.selected.is_some() {
            // A clicked game fills the page until a system or All games is picked again.
            return page.child(div().flex_1().min_h_0().flex().child(self.details_pane(cx)));
        }
        page.child(self.game_list(cx))
    }

    /// The library's covers.
    fn game_list(&self, cx: &mut Context<Self>) -> AnyElement {
        if self.visible_cache.is_empty() {
            return div().px_6().text_color(rgb(MUTED)).child("No games here.").into_any_element();
        }
        let rows = self.visible_cache.len().div_ceil(self.columns.max(1));
        uniform_list("grid", rows, cx.processor(|view: &mut Hub, range: std::ops::Range<usize>, _, cx| range.map(|row| view.grid_row(row, cx)).collect::<Vec<_>>()))
            .flex_1()
            .track_scroll(self.grid_scroll.clone())
            .into_any_element()
    }

    /// The sidebar's games, grouped by system under collapsible headers.
    fn side_list(&self, cx: &mut Context<Self>) -> AnyElement {
        if self.rows_cache.is_empty() {
            return div().px_3().text_color(rgb(MUTED)).child("No games here.").into_any_element();
        }
        uniform_list("side-list", self.rows_cache.len(), cx.processor(|view: &mut Hub, range: std::ops::Range<usize>, _, cx| range.map(|row| view.side_row(row, cx)).collect::<Vec<_>>()))
            .flex_1()
            .track_scroll(self.side_scroll.clone())
            .into_any_element()
    }

    /// Asks libretro for the selected game's screenshot once, for its page's background art.
    fn fetch_selected_snap(&mut self, cx: &mut Context<Self>) {
        let Some(index) = self.selected else { return };
        let game = self.games[index].clone();
        if !self.snaps_tried.insert(game.path.clone()) {
            return;
        }
        let (key, tgdb) = (self.config.steamgriddb_key.clone(), self.config.thegamesdb_key.clone());
        cx.spawn(async move |this, cx| {
            let found = cx
                .background_executor()
                .spawn(async move {
                    let snap = emu::fetch_snap(&game);
                    // Heroes, logos and icons for the game being looked at, one game at a time.
                    let art = [emu::Art::Hero, emu::Art::Logo, emu::Art::Icon].map(|kind| emu::fetch_art(&game, kind, &key, &tgdb));
                    let icon = art[2].then(|| emu::art_path(&game, emu::Art::Icon));
                    (snap || art.contains(&true), game.path, icon)
                })
                .await;
            let _ = this.update(cx, |view, cx| {
                let (any, path, icon) = found;
                if let Some(i) = view.games.iter().position(|g| g.path == path) {
                    if let Some(slot) = view.icons.get_mut(i) {
                        *slot = icon;
                    }
                }
                if any {
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// A game's page in the library: background art fading into the page, then the title, buttons and a facts table.
    fn details_pane(&self, cx: &mut Context<Self>) -> Div {
        // Its own solid fill: the art fades into this color, so the see-through library page mustn't show below it.
        let pane = div().flex_1().min_w_0().h_full().flex().flex_col().overflow_hidden().bg(self.appearance.fill(0x0c1417));
        let Some(index) = self.selected else {
            return pane.items_center().justify_center().text_color(rgb(MUTED)).child("Select a game to see it here.");
        };
        let game = &self.games[index];
        let (hero, snap) = (emu::art_path(game, emu::Art::Hero), emu::snap_path(game));
        let art = if hero.exists() {
            Some(hero)
        } else if snap.exists() {
            Some(snap)
        } else {
            self.covers[index].clone()
        };
        let page = self.appearance.fill(0x0c1417);
        let fade = linear_gradient(180., linear_color_stop(Rgba { a: 0., ..page }, 0.3), linear_color_stop(page, 1.));
        pane.child(
            div()
                .relative()
                // Most of the page, growing with the window.
                .h(relative(0.65))
                .min_h(px(300.))
                .flex_none()
                .overflow_hidden()
                .children(art.map(|art| div().absolute().inset_0().opacity(0.6).child(img(art).size_full().object_fit(ObjectFit::Cover))))
                // The logo sits on the background's right side, clear of the title and buttons.
                .children(Some(emu::art_path(game, emu::Art::Logo)).filter(|l| l.exists()).map(|logo| {
                    div().absolute().top(px(24.)).right(px(32.)).w(px(280.)).h(px(140.)).child(img(logo).size_full().object_fit(ObjectFit::Contain))
                }))
                .child(div().absolute().inset_0().bg(fade)),
        )
        .child(
            div()
                .id("details-pane")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .px_8()
                .pb_8()
                .flex()
                .flex_col()
                .gap_4()
                // Pulled up over the art's fade, like Playnite's title.
                .mt(px(-56.))
                .child(div().text_size(px(30.)).font_weight(FontWeight::SEMIBOLD).line_clamp(2).child(game.name.clone()))
                .child(self.game_actions(index, cx))
                .child(self.game_facts(index).max_w(px(520.))),
        )
    }

    /// Play, More and Favorite, shared by the game's page and Home's side panel.
    fn game_actions(&self, index: usize, cx: &mut Context<Self>) -> Div {
        let favorite = self.favorites.contains(&self.games[index].path);
        div()
            .flex()
            .flex_wrap()
            .gap_2()
            .child(Button::new("game-play").primary().label("Play").min_w(px(110.)).on_click(cx.listener(move |view, _, _, cx| view.play(index, cx))))
            .child(Button::new("game-more").label("More").min_w(px(110.)).on_click(cx.listener(move |view, _, _, cx| view.open_details(index, cx))))
            .child(Button::new("game-favorite").label(if favorite { "Unfavorite" } else { "Favorite" }).on_click(cx.listener(move |view, _, _, cx| {
                view.toggle_favorite(index);
                cx.notify();
            })))
    }

    /// The Details table: play time, last played, platform, favorite, file.
    fn game_facts(&self, index: usize) -> Div {
        let game = &self.games[index];
        let fact = |label: &str, value: String| {
            div().flex().gap_4().py_1().child(div().w(px(100.)).flex_none().text_color(rgb(MUTED)).child(label.to_owned())).child(div().min_w_0().truncate().child(value))
        };
        let file = game.path.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default();
        div()
            .flex()
            .flex_col()
            .child(div().pb_2().child(heading("Details")))
            .child(fact("Time played", self.playtime.get(&game.path).and_then(|&s| emu::playtime_text(s)).unwrap_or("Never".into())))
            .child(fact("Last played", self.history.get(&game.path).map_or("Never".into(), |&t| emu::ago(t))))
            .child(fact("Platform", system_name(&self.systems, &game.system).to_owned()))
            .child(fact("Favorite", if self.favorites.contains(&game.path) { "Yes" } else { "No" }.into()))
            .child(fact("File", file))
    }

    /// The Settings popup: categories on the left, the chosen category's card on the right.
    fn settings_popup(&self, cx: &mut Context<Self>) -> Div {
        let paths = card()
            .child(setting_row("ROMs folder", "One subfolder per system, EmuDeck style (roms/snes, roms/gba…).").child(div().w(px(360.)).child(Input::new(&self.roms_dir))))
            .child(setting_row("RetroArch command", "Program and arguments, separated by spaces.").child(div().w(px(360.)).child(Input::new(&self.retroarch_cmd))))
            .child(setting_row("Cores folder", "Where RetroArch keeps its cores.").child(div().w(px(360.)).child(Input::new(&self.cores_dir))))
            .child(setting_row("SteamGridDB API key", "Optional. Fills in covers libretro doesn't have. steamgriddb.com → Preferences → API.").child(div().w(px(360.)).child(Input::new(&self.steamgriddb_key).mask_toggle())))
            .child(setting_row("TheGamesDB API key", "Optional. Box art, backgrounds and logos for retro games. Request one on forums.thegamesdb.net.").child(div().w(px(360.)).child(Input::new(&self.thegamesdb_key).mask_toggle())))
            .child(setting_row("Save changes", "").child(
                Button::new("save").primary().label("Save").on_click(cx.listener(|view, _, _, cx| view.save_settings(cx))),
            ));

        let folders_exist = self.config.roms_dir.is_dir();
        let retroarch = emu::retroarch_installed(&self.config);
        let retroarch_warning = emu::retroarch_version_warning(&self.config);
        let (missing, total) = emu::core_status(&self.config);
        // Only systems with games, so empty EmuDeck folders don't fill the list.
        let missing_bios: Vec<String> = self
            .systems
            .iter()
            .filter(|s| self.games.iter().any(|g| g.system == s.id))
            .filter_map(|s| {
                let files = emu::missing_bios(&self.config, s);
                (!files.is_empty()).then(|| format!("{} ({})", s.name, files.join(", ")))
            })
            .collect();
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
                if let Some(warning) = &retroarch_warning {
                    setting_row("RetroArch", warning)
                } else if retroarch {
                    setting_row("RetroArch", "Installed. Open it to change its own settings.")
                } else {
                    setting_row("RetroArch", "Not installed. Installs with winget, Homebrew or Flatpak.")
                }
                .child(self.setting_button(
                    Setting::RetroArch,
                    Button::new("retroarch").label(if retroarch_warning.is_some() { "Download" } else if retroarch { "Open" } else { "Install" }).disabled(self.busy && !retroarch),
                    cx,
                )),
            )
            .child(
                setting_row(
                    "Cores",
                    &format!(
                        "{} Optional alternate cores are in the list, installed one by one.",
                        if missing == 0 { format!("All {total} main cores installed.") } else { format!("{missing} of {total} main cores missing. Install all downloads them from libretro.") }
                    ),
                )
                .child(
                    div()
                        .flex()
                        .gap_2()
                        .child(self.setting_button(Setting::CoreList, Button::new("core-list").label("Open list"), cx))
                        .child(self.setting_button(
                            Setting::Cores,
                            Button::new("cores").label(if missing == 0 { "All installed" } else { "Install all" }).disabled(self.busy || missing == 0),
                            cx,
                        )),
                ),
            )
            .child(setting_row(
                "BIOS files",
                &if missing_bios.is_empty() {
                    "Every system with games has its BIOS files.".to_owned()
                } else {
                    format!("Missing in {}: {}", emu::emulation_folder(&self.config).join("bios").display(), missing_bios.join("; "))
                },
            ))
            .child(
                setting_row("Steam games", "Installed Steam games in PC Games. They start through Steam.")
                    .child(self.setting_button(Setting::SteamGames, Button::new("steam-games").label(if self.config.hide_steam { "Off" } else { "On" }).selected(!self.config.hide_steam), cx)),
            )
            .child(
                setting_row(
                    "PC game folders",
                    &format!(
                        "Programs and shortcuts, or one subfolder per game. Always scans {}{}",
                        self.config.roms_dir.join(emu::PC).display(),
                        self.config.pc_folders.iter().map(|f| format!(", {}", f.display())).collect::<String>(),
                    ),
                )
                .child(
                    div()
                        .flex()
                        .gap_2()
                        .child(self.setting_button(Setting::AddPcFolder, Button::new("add-pc-folder").label("Add folder…"), cx))
                        .child(self.setting_button(Setting::ClearPcFolders, Button::new("clear-pc-folders").label("Clear").disabled(self.config.pc_folders.is_empty()), cx)),
                ),
            )
            .child({
                let hidden = self.game_options.values().filter(|o| o.hidden).count();
                setting_row("Hidden games", &format!("{hidden} hidden from their game pages."))
                    .child(self.setting_button(Setting::UnhideGames, Button::new("unhide-games").label("Unhide all").disabled(hidden == 0), cx))
            })
            .child({
                let updated = emu::launchbox_updated();
                let note = match updated {
                    Some(when) => format!("Covers, logos and backgrounds with no API key. Downloaded {}.", emu::ago(when)),
                    None => "Covers, logos and backgrounds with no API key. Downloads LaunchBox's games database once (about 110 MB) and keeps a small index.".to_owned(),
                };
                setting_row("LaunchBox art", &note).child(self.setting_button(
                    Setting::LaunchBox,
                    Button::new("launchbox").label(if updated.is_some() { "Refresh" } else { "Download" }).disabled(self.busy),
                    cx,
                ))
            });

        let mut cores = card();
        for (i, core) in CORES.iter().enumerate() {
            let names = |pick: &dyn Fn(&emu::System) -> bool| self.systems.iter().filter(|s| pick(s)).map(|s| s.name.as_str()).collect::<Vec<_>>().join(", ");
            let (main, alternate) = (names(&|s| &s.core == core), names(&|s| s.alts.contains(core)));
            let installed = emu::core_installed(&self.config, core);
            let mut note = vec![(if installed { "Installed." } else { "Not installed." }).to_owned()];
            if !main.is_empty() {
                note.push(format!("Main core for {main}."));
            } else if MAIN_CORES.contains(core) {
                // The Zapper core isn't any system's main core, but Install all gets it.
                note.push("Used for NES light-gun games.".to_owned());
            }
            if !alternate.is_empty() {
                note.push(format!("Optional alternate for {alternate}."));
            }
            let note = note.join(" ");
            cores = cores.child(setting_row(core, &note).child(self.setting_button(
                Setting::Core(i),
                Button::new(("core", i)).label(if installed { "Reinstall" } else { "Install" }).disabled(self.busy),
                cx,
            )));
        }

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

        let mut palettes = div().flex().flex_wrap().justify_end().gap_2();
        for (i, (name, surface, _)) in PALETTES.into_iter().enumerate() {
            palettes = palettes.child(self.setting_button(
                Setting::Palette(i),
                Button::new(name).label(name).selected(self.appearance.surface == surface),
                cx,
            ));
        }
        let mut corners = div().flex().gap_2();
        for (label, square) in [("Rounded", false), ("Square", true)] {
            corners = corners.child(self.setting_button(
                Setting::SquareCovers(square),
                Button::new(label).label(label).selected(self.appearance.square_covers == square),
                cx,
            ));
        }
        let mut backdrops = div().flex().gap_2();
        for (label, backdrop) in [("Solid", appearance::Backdrop::Solid), ("Color", appearance::Backdrop::Color), ("Cover", appearance::Backdrop::Cover)] {
            backdrops = backdrops.child(self.setting_button(
                Setting::Backdrop(backdrop),
                Button::new(label).label(label).selected(self.appearance.backdrop == backdrop),
                cx,
            ));
        }
        let looks = card()
            .child(setting_row("Theme", "Preset colors for the whole window.").child(palettes))
            .child(setting_row("Background", "Behind the game in view: plain, its cover's color, or its faded cover.").child(backdrops))
            .child(setting_row("Cover corners", "Rounded or square game covers.").child(corners))
            .child(setting_row(
                &format!("EmuHub {}", env!("CARGO_PKG_VERSION")),
                &match &self.update {
                    Some(release) => format!("Version {} is available.", release.version),
                    None => "Up to date, or the update check couldn't reach GitHub.".into(),
                },
            )
            .child(self.setting_button(
                Setting::Update,
                Button::new("update").label(if self.update.is_some() { "Update" } else { "Releases" }).selected(self.update.is_some()),
                cx,
            )));

        let body = match self.settings_cat {
            0 => setup,
            1 => cores,
            2 => emulators,
            3 => looks,
            _ => paths,
        };
        let mut categories = div().w(px(200.)).flex_none().p_3().flex().flex_col().gap_1().border_r_1().border_color(rgba(0xffffff14));
        categories = categories.child(div().px_3().pt_2().pb_3().text_size(px(22.)).font_weight(FontWeight::SEMIBOLD).child("Settings"));
        for (c, name) in SETTING_CATEGORIES.into_iter().enumerate() {
            categories = categories.child(
                self.ring(self.nav_row(("settings-cat", c), self.settings_cat == c), self.focus == Focus::SettingsCat(c))
                    .child(name)
                    .on_click(cx.listener(move |view, _, _, cx| {
                        view.settings_cat = c;
                        view.settings_scroll.scroll_to_item(0);
                        cx.notify();
                    })),
            );
        }
        let content = div()
            .flex_1()
            .min_w_0()
            .p_6()
            .flex()
            .flex_col()
            .gap_4()
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(div().text_size(px(22.)).font_weight(FontWeight::SEMIBOLD).child(SETTING_CATEGORIES[self.settings_cat]))
                    .child(Button::new("settings-close").label("Close").on_click(cx.listener(|view, _, _, cx| {
                        view.close_settings();
                        cx.notify();
                    }))),
            )
            // card() is flex_none; let it shrink to the space left here so a long card scrolls.
            .child(div().flex_1().min_h_0().flex().flex_col().child(body.id("settings-card").flex_shrink().min_h_0().overflow_y_scroll().track_scroll(&self.settings_scroll)));
        // Dims the page behind.
        div()
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .bg(rgba(0x00000099))
            .occlude()
            .child(
                div()
                    .w(px(920.))
                    .max_w(relative(0.94))
                    .h(relative(0.86))
                    .max_h(px(640.))
                    .flex()
                    .rounded_lg()
                    .overflow_hidden()
                    .border_1()
                    .border_color(rgba(0xffffff1f))
                    .bg(self.appearance.fill(0x0c1417))
                    .shadow_lg()
                    .child(categories)
                    .child(content),
            )
    }

    // ---- Ctrl+K search ------------------------------------------------------

    /// Games matching the search across the whole library; recently played ones while it's empty.
    fn search_results(&self, cx: &App) -> Vec<usize> {
        let query = self.query.read(cx).value().to_lowercase();
        if query.is_empty() {
            return self.recent().into_iter().take(SEARCH_RESULTS).collect();
        }
        (0..self.games.len()).filter(|&i| self.games[i].name.to_lowercase().contains(&query)).take(SEARCH_RESULTS).collect()
    }

    fn open_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.search = Some(0);
        self.query.update(cx, |input, cx| {
            input.set_value("", window, cx);
            input.focus(window, cx);
        });
        cx.notify();
    }

    /// Closes the popup and clears its text, which also filters the library while it's open.
    fn close_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.search = None;
        self.query.update(cx, |input, cx| input.set_value("", window, cx));
        window.blur();
    }

    /// Enter, X or a click: the picked game's page.
    fn search_pick(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(index) = self.search.and_then(|pick| self.search_results(cx).get(pick).copied()) {
            self.close_search(window, cx);
            self.show_game(index);
        }
    }

    fn search_popup(&self, cx: &mut Context<Self>) -> Div {
        let pick = self.search.unwrap_or(0);
        let results = self.search_results(cx);
        let mut list = div().flex().flex_col().gap_1().p_2();
        if results.is_empty() {
            list = list.child(div().p_3().text_color(rgb(MUTED)).child("No games found."));
        }
        for (n, &index) in results.iter().enumerate() {
            let game = &self.games[index];
            list = list.child(
                self.nav_row(("search", n), n == pick)
                    .h(px(56.))
                    .child(div().w(px(32.)).h(px(44.)).flex_none().rounded_sm().overflow_hidden().child(self.cover(index, 32.)))
                    .child(div().flex_1().min_w_0().truncate().child(game.name.clone()))
                    .child(div().text_xs().text_color(rgb(MUTED)).child(system_name(&self.systems, &game.system).to_owned()))
                    .on_click(cx.listener(move |view, _, window, cx| {
                        view.search = Some(n);
                        view.search_pick(window, cx);
                    })),
            );
        }
        let panel = div()
            .w(px(620.))
            .max_w(relative(0.94))
            .rounded_lg()
            .overflow_hidden()
            .border_1()
            .border_color(rgba(0xffffff1f))
            .bg(self.appearance.fill(0x0c1417))
            .shadow_lg()
            // Clicks inside don't reach the backdrop, which closes the search.
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(div().p_3().border_b_1().border_color(rgba(0xffffff14)).child(Input::new(&self.query).prefix(icon("search", 16., rgb(MUTED)))))
            .child(list);
        div()
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .flex()
            .items_start()
            .justify_center()
            .pt(px(96.))
            .bg(rgba(0x00000099))
            .occlude()
            .on_mouse_down(MouseButton::Left, cx.listener(|view, _, window, cx| {
                view.close_search(window, cx);
                cx.notify();
            }))
            // Ahead of the input, so arrows, Enter and Escape drive the results.
            .capture_key_down(cx.listener(|view, event: &KeyDownEvent, window, cx| {
                let len = view.search_results(cx).len();
                let pick = view.search.unwrap_or(0);
                match event.keystroke.key.as_str() {
                    "up" => view.search = Some(pick.saturating_sub(1)),
                    "down" => view.search = Some((pick + 1).min(len.saturating_sub(1))),
                    "enter" => view.search_pick(window, cx),
                    "escape" => view.close_search(window, cx),
                    _ => return,
                }
                cx.stop_propagation();
                cx.notify();
            }))
            .child(panel)
    }

    fn save_appearance(&mut self, cx: &mut Context<Self>) {
        self.appearance.apply_theme(cx);
        self.message = match self.appearance.save() {
            Ok(()) => "Appearance saved.".into(),
            Err(error) => error,
        };
        cx.notify();
    }

    // ---- home -------------------------------------------------------------

    /// Played games, most recent first (the Continue Playing row).
    /// The color of the game in view (game page, selected game, or Home's hero), working it out in the background once.
    /// The cover of the game in view: game page, selected game, or Home's hero.
    fn ambient_cover(&self) -> Option<std::path::PathBuf> {
        let index = match (&self.details, self.selected, self.view, self.focus) {
            (Some(d), ..) => d.game,
            (None, Some(i), ..) => i,
            (None, None, View::Home, Focus::Home(0, c)) => *self.recent().get(c)?,
            (None, None, View::Home, Focus::Home(1, c)) => *self.favorite_games().get(c)?,
            (None, None, View::Home, _) => self.recent().first().copied().filter(|&i| self.covers[i].is_some()).or_else(|| self.idle_game())?,
            _ => self.idle_game()?,
        };
        self.covers.get(index)?.clone()
    }

    /// The color of the game in view, working it out in the background once.
    fn ambient_tint(&mut self, cx: &mut Context<Self>) -> Option<u32> {
        let cover = self.ambient_cover()?;
        if let Some(&tint) = self.tints.get(&cover) {
            return tint;
        }
        self.tints.insert(cover.clone(), None);
        cx.spawn(async move |this, cx| {
            let path = cover.clone();
            let tint = cx.background_executor().spawn(async move { emu::cover_color(&path) }).await;
            let _ = this.update(cx, |view, cx| {
                view.tints.insert(cover, tint);
                cx.notify();
            });
        })
        .detach();
        None
    }

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

    /// Favorite games in library order (the Favorites row).
    /// The whole banner, never cropped: shrunk to fit the hero, on its own edge color so the bars don't show.
    fn banner_art(&self) -> AnyElement {
        div().size_full().bg(rgb(0x0f051a)).child(img(self.banner.clone()).size_full().object_fit(ObjectFit::Contain)).into_any_element()
    }

    /// The background's game when none is selected: a random favorite with a cover, else the first game with a cover.
    /// None only while no game has a cover yet; the page's plain background shows then.
    fn idle_game(&self) -> Option<usize> {
        let favorites: Vec<usize> = self.favorite_games().into_iter().filter(|&i| self.covers[i].is_some()).collect();
        favorites.get(self.idle_seed % favorites.len().max(1)).copied().or_else(|| self.covers.iter().position(Option::is_some))
    }

    fn favorite_games(&self) -> Vec<usize> {
        (0..self.games.len()).filter(|&i| self.favorites.contains(&self.games[i].path)).collect()
    }

    /// The games in a Home game row: 0 Continue Playing, 1 Favorites.
    fn home_games(&self, row: usize) -> Vec<usize> {
        if row == 0 { self.recent() } else { self.favorite_games() }
    }

    fn home_row_len(&self, row: usize) -> usize {
        if row < 2 { self.home_games(row).len() } else { self.home_systems().len() }
    }

    /// First Home row with something in it, for entering Home with the controller.
    fn enter_home(&mut self) {
        if let Some(row) = (0..3).find(|&r| self.home_row_len(r) > 0) {
            self.focus = Focus::Home(row, 0);
            self.home_scroll[row].scroll_to_item(0);
            self.scroll_home_to(row);
        }
    }

    /// Scrolls Home so row `row` is visible; the top row also brings the hero back into view.
    fn scroll_home_to(&self, row: usize) {
        // Page children: hero, divider, then each row that's shown (Systems always is).
        let shown: Vec<usize> = (0..3).filter(|&r| r == 2 || self.home_row_len(r) > 0).collect();
        let pos = shown.iter().position(|&r| r == row).unwrap_or(0);
        self.home_page_scroll.scroll_to_item(if pos == 0 { 0 } else { 2 + pos });
        if pos > 0 && pos + 1 == shown.len() {
            // scroll_to_item only reveals the row's top part when it's taller than what's left.
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
                let other = if action == Pad::Up {
                    (0..row).rev().find(|&r| self.home_row_len(r) > 0)
                } else {
                    (row + 1..3).find(|&r| self.home_row_len(r) > 0)
                };
                if let Some(other) = other {
                    let col = col.min(self.home_row_len(other) - 1);
                    self.focus = Focus::Home(other, col);
                    self.home_scroll[other].scroll_to_item(col);
                    self.scroll_home_to(other);
                }
            }
            Pad::Confirm if row < 2 => {
                if let Some(&game) = self.home_games(row).get(col) {
                    self.play(game, cx);
                }
            }
            Pad::Options if row < 2 => {
                if let Some(&game) = self.home_games(row).get(col) {
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
            // overflow_hidden doesn't clip to rounded corners in GPUI, so the image is rounded itself.
            Some(cover) => img(cover.clone()).size_full().rounded(self.cover_corners()).object_fit(ObjectFit::Cover).into_any_element(),
            None => tile(&self.games[index].name, size).size_full().rounded(self.cover_corners()).into_any_element(),
        }
    }

    /// One row of the sidebar list, the same height on every row for uniform_list: a system header or a game.
    fn side_row(&self, row: usize, cx: &mut Context<Self>) -> Div {
        match &self.rows_cache[row] {
            LibRow::Game(index) => self.list_row(*index, cx),
            LibRow::Header(system, count) => {
                let open = self.expanded.contains(system) || !self.query.read(cx).value().is_empty();
                let shown = self.view == View::Library && self.system.as_ref() == Some(system) && !self.showing_game;
                let focused = self.menu().iter().position(|m| *m == MenuItem::Library(Some(system.clone()))).is_some_and(|i| self.focus == Focus::Menu(i));
                let pick = system.clone();
                // Styled like the Home and All games buttons above it.
                let header = self
                    .ring(self.nav_row(SharedString::from(format!("group-{system}")), shown), focused)
                    .h(px(32.))
                    .w_full()
                    .child(icon(if open { "chevron-down" } else { "chevron-right" }, 16., rgb(if shown { TEXT } else { MUTED })))
                    .child(div().flex_1().min_w_0().truncate().child(system_name(&self.systems, system).to_owned()))
                    .child(div().text_xs().text_color(rgb(MUTED)).child(count.to_string()))
                    .on_click(cx.listener(move |view, _, _, cx| {
                        view.pick_system(&pick);
                        cx.notify();
                    }));
                // 34px like a game row, which uniform_list needs.
                div().w_full().h(px(34.)).px_2().py_px().child(header)
            }
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
                        view.show_game(index);
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
                            .rounded(self.cover_corners())
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

    /// One game in the sidebar's list, the same height on every row for uniform_list.
    fn list_row(&self, index: usize, cx: &mut Context<Self>) -> Div {
        let game = &self.games[index];
        let selected = self.selected == Some(index);
        let accent = self.appearance.accent;
        let row = div()
            .id(("game", index))
            .h(px(30.))
            .flex()
            .items_center()
            .gap_3()
            .px_3()
            .rounded_md()
            .cursor_pointer()
            .border_1()
            // Bright when the controller is in the list, dim while it's in the side menu.
            .border_color(match (selected, self.focus == Focus::Grid) {
                (true, true) => rgb(accent).into(),
                (true, false) => rgba((accent << 8) | 0x60),
                _ => rgba(0x00000000),
            })
            .bg(if selected { rgba((accent << 8) | 0x20) } else { rgba(0x00000000) })
            .hover(|style| style.bg(rgba(0xffffff0a)))
            .child(match self.icons.get(index).cloned().flatten() {
                Some(icon) => div().w(px(18.)).h(px(24.)).flex_none().flex().items_center().child(img(icon).size(px(18.)).object_fit(ObjectFit::Contain)),
                None => div().w(px(18.)).h(px(24.)).flex_none().rounded_sm().overflow_hidden().child(self.cover(index, 18.)),
            })
            .child(div().flex_1().min_w_0().truncate().child(game.name.clone()))
            .children(self.favorites.contains(&game.path).then(|| icon("heart-filled", 14., rgb(accent))))
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |view, _: &MouseDownEvent, _, cx| {
                    view.focus = Focus::Grid;
                    view.open_details(index, cx);
                }),
            )
            .on_click(cx.listener(move |view, event: &ClickEvent, _, cx| {
                view.show_game(index);
                if event.click_count() >= 2 {
                    view.play(index, cx);
                }
                cx.notify();
            }));
        div().px_2().py_0p5().child(row)
    }

    /// A Home row of covers (Continue Playing or Favorites): click selects, double-click plays, right-click opens the game page.
    fn game_row(&self, row: usize, games: &[usize], cx: &mut Context<Self>) -> Stateful<Div> {
        let mut cards = div().id(("game-row", row)).flex().gap_4().px_6().pb_2().overflow_x_scroll().track_scroll(&self.home_scroll[row]);
        for (col, &index) in games.iter().enumerate() {
            cards = cards.child(
                self.ring(div().id(("home-game", row * 100_000 + col)).w(px(120.)).h(px(180.)).flex_none().rounded(self.cover_corners()).overflow_hidden(), self.focus == Focus::Home(row, col))
                    .cursor_pointer()
                    .bg(rgba(0xffffff0a))
                    .child(self.cover(index, 120.))
                    .on_mouse_down(
                        MouseButton::Right,
                        cx.listener(move |view, _: &MouseDownEvent, _, cx| {
                            view.focus = Focus::Home(row, col);
                            view.open_details(index, cx);
                        }),
                    )
                    // Opens the game's page, like clicking a game anywhere else.
                    .on_click(cx.listener(move |view, _, _, cx| {
                        view.show_game(index);
                        cx.notify();
                    })),
            );
        }
        cards
    }

    /// Gray stand-ins for the hero and two rows of covers, gently pulsing, while the first scan runs.
    fn skeleton_page(&self) -> Stateful<Div> {
        let block = |w: f32, h: f32| div().w(px(w)).h(px(h)).flex_none().rounded(self.cover_corners()).bg(rgba(0xffffff0f));
        let row = |n: usize| div().flex().gap_4().px_6().pb_6().children((0..n).map(|_| block(150., 225.)));
        let content = div()
            .flex()
            .flex_col()
            .child(div().p_6().flex().flex_col().gap_3().child(block(420., 40.)).child(block(260., 18.)).child(block(180., 18.)))
            .child(div().px_6().pt_5().pb_2().child(block(180., 18.)))
            .child(row(8))
            .child(div().px_6().pt_5().pb_2().child(block(140., 18.)))
            .child(row(8))
            .with_animation("skeleton", Animation::new(std::time::Duration::from_millis(1200)).repeat().with_easing(pulsating_between(0.4, 1.)), |d, t| {
                d.opacity(t)
            });
        div().id("skeleton").size_full().overflow_hidden().bg(self.appearance.fill(0x0c1417)).child(content)
    }

    fn home_page(&self, cx: &mut Context<Self>) -> Stateful<Div> {
        let recent = self.recent();
        let favorites = self.favorite_games();
        let hero_index = match self.focus {
            Focus::Home(0, c) => recent.get(c),
            Focus::Home(1, c) => favorites.get(c),
            _ => recent.first(),
        }
        .copied();
        let hero = hero_index.map(|i| &self.games[i]);

        // Hero: in-game screenshot on the left, title and facts on the right, like the old EmuHub's showcase.
        let hero_art = match hero {
            Some(game) if emu::art_path(game, emu::Art::Hero).exists() => {
                img(emu::art_path(game, emu::Art::Hero)).size_full().object_fit(ObjectFit::Cover).into_any_element()
            }
            Some(game) if emu::snap_path(game).exists() => {
                img(emu::snap_path(game)).size_full().object_fit(ObjectFit::Cover).into_any_element()
            }
            // No art at all (not even a cover): EmuHub's banner rather than a letter tile on a plain background.
            Some(_) if hero_index.is_some_and(|i| self.covers[i].is_none()) => {
                self.banner_art()
            }
            // No screenshot yet: show the whole box, centered, rather than a stretched crop of it.
            Some(_) => div()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .bg(rgba(0x00000040))
                .child(div().w(px(180.)).h(px(270.)).rounded(self.cover_corners()).overflow_hidden().child(self.cover(hero_index.unwrap(), 180.)))
                .into_any_element(),
            None => self.banner_art(),
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
                            .child(self.history.get(&game.path).map_or("Not played yet".into(), |&t| emu::ago(t))),
                    )
                    .children(self.playtime.get(&game.path).and_then(|&s| emu::playtime_text(s)).map(|time| {
                        div().flex().items_center().gap_2().child(icon("play", 16., rgb(MUTED))).child(time)
                    }))
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
        // The hero stays solid; the rows below are see-through to the window's background, like the library.
        let hero = div()
            .flex()
            .h(px(300.))
            .flex_none()
            .bg(self.appearance.fill(0x0c1417))
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
                    .child(match hero.map(|g| emu::art_path(g, emu::Art::Logo)).filter(|p| p.exists()) {
                        Some(logo) => img(logo).w(px(320.)).h(px(90.)).object_fit(ObjectFit::Contain).into_any_element(),
                        None => div().text_size(px(30.)).font_weight(FontWeight::SEMIBOLD).text_center().line_clamp(2).child(title).into_any_element(),
                    })
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

        let mut systems_row = div().id("systems-row").flex().gap_4().px_6().pb_6().overflow_x_scroll().track_scroll(&self.home_scroll[2]);
        for (col, system) in self.home_systems().into_iter().enumerate() {
            let count = self.games.iter().filter(|g| g.system == system.id).count();
            let id = system.id.clone();
            systems_row = systems_row.child(
                self.ring(div().id(("system", col)).w(px(150.)).h(px(72.)).flex_none().rounded_lg(), self.focus == Focus::Home(2, col))
                    .cursor_pointer()
                    .bg(rgba(0xffffff0a))
                    .hover(|style| style.bg(rgba(0xffffff14)))
                    .p_3()
                    .flex()
                    .flex_col()
                    .justify_center()
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

        let focused_name = |row: usize, games: &[usize]| match self.focus {
            Focus::Home(r, c) if r == row => games.get(c).map(|&i| self.games[i].name.clone()),
            _ => None,
        };
        let mut page = div()
            .id("home")
            .size_full()
            .overflow_y_scroll()
            .track_scroll(&self.home_page_scroll)
            .flex()
            .flex_col()
            .child(hero)
            .child(div().h(px(1.)).bg(rgba((accent << 8) | 0x30)));
        for (row, title, games) in [(0, "Continue Playing", &recent), (1, "Favorites", &favorites)] {
            if !games.is_empty() {
                page = page.child(div().child(row_title(title, focused_name(row, games).as_deref())).child(self.game_row(row, games, cx)));
            }
        }
        page.child(div().child(row_title("Systems", None)).child(systems_row))
    }

    // ---- button hints -----------------------------------------------------

    /// What the controller buttons do right now, shown in the bottom bar.
    fn hints(&self) -> Vec<(Hint, &'static str)> {
        if self.details.is_some() {
            return vec![(Hint::Confirm, "Select"), (Hint::Back, "Back")];
        }
        if self.search.is_some() {
            return vec![(Hint::Confirm, "Open"), (Hint::Back, "Close")];
        }
        match self.focus {
            Focus::Menu(_) => vec![(Hint::Confirm, "Open"), (Hint::Options, "Search"), (Hint::Bumpers, "Switch system")],
            Focus::Grid | Focus::Home(0 | 1, _) => vec![(Hint::Confirm, "Play"), (Hint::Options, "Details"), (Hint::Back, "Back")],
            Focus::Home(..) => vec![(Hint::Confirm, "Open"), (Hint::Back, "Back")],
            Focus::Settings(_) | Focus::SettingsCat(_) => vec![(Hint::Confirm, "Select"), (Hint::Back, "Back")],
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
            // Same width as the message on the right, so the hints stay centered.
            .child(div().w(px(300.)))
            .child(self.hint_bar())
            .child(div().w(px(300.)).text_xs().text_right().text_color(rgb(MUTED)).child(format!("{} games", self.games.len())))
    }

    /// The latest message as a card above the bottom bar's right end, sliding up as it appears,
    /// sized and styled like Sonora's (Sonner-style) toasts.
    fn toast(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if self.toast.0 != self.message {
            self.toast = (self.message.clone(), std::time::Instant::now(), self.toast.2 + 1);
        }
        if self.message.is_empty() {
            return None;
        }
        let close = div()
            .id("toast-close")
            .flex_none()
            .p_1()
            .rounded_md()
            .cursor_pointer()
            .hover(|d| d.bg(rgba(0xffffff14)))
            .child(icon("btn-cross", 14., rgb(MUTED)))
            .on_click(cx.listener(|view, _, _, cx| {
                view.message.clear();
                cx.notify();
            }));
        let card = div()
            .absolute()
            .bottom(px(76. + 14.))
            .right(px(14.))
            .w(px(364.))
            .flex()
            .items_center()
            .gap_3()
            .pl_4()
            .pr_2()
            .py_3()
            .rounded_lg()
            .bg(self.appearance.fill(0x101e23))
            .border_1()
            .border_color(rgba(0xffffff1f))
            .shadow_lg()
            .child(div().flex_1().min_w_0().child(self.message.clone()))
            .child(close);
        Some(
            card.with_animation(("toast", self.toast.2), Animation::new(std::time::Duration::from_millis(400)).with_easing(ease_out_quint()), |card, t| {
                card.bottom(px(76. + 14. - 16. * (1. - t))).opacity(t)
            })
            .into_any_element(),
        )
    }
}

impl Render for Hub {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let toast = self.toast(cx);
        let (tint, backdrop_cover) = match self.appearance.backdrop {
            appearance::Backdrop::Solid => (None, None),
            appearance::Backdrop::Color => (self.ambient_tint(cx), None),
            // The game page draws its own faded cover.
            // With no cover to show yet, EmuHub's own banner, like Home's hero before anything is played.
            appearance::Backdrop::Cover if self.details.is_none() => {
                (None, Some(self.ambient_cover().map(ImageSource::from).unwrap_or_else(|| self.banner.clone().into())))
            }
            appearance::Backdrop::Cover => (None, None),
        };
        let on_page = match self.focus {
            Focus::Menu(i) => i < self.menu().len(),
            Focus::Grid => self.view == View::Library,
            Focus::Settings(_) | Focus::SettingsCat(_) => self.settings_open,
            Focus::Home(..) => self.view == View::Home,
        };
        if self.settings_open && !matches!(self.focus, Focus::Settings(_) | Focus::SettingsCat(_)) {
            // Opened with the mouse: the controller drives the popup, not the page behind it.
            self.focus = Focus::SettingsCat(self.settings_cat);
        } else if !on_page {
            self.focus = Focus::Menu(self.current_menu());
        }
        // Sidebar, window padding, border and grid padding; each card is 150px plus a 20px gap.
        let chrome = if self.sidebar_open { 320. } else { 0. } + 16. + 4. + 2. + 48.;
        let content = window.viewport_size().width / px(1.) - chrome;
        self.columns = ((content + 20.) / 170.).floor().max(1.) as usize;
        let visible = self.visible(cx);
        self.visible_cache = visible.clone();
        self.rows_cache = self.lib_rows(cx);
        if self.showing_game {
            self.fetch_selected_snap(cx);
        }
        let page = match self.view {
            _ if !self.scanned => self.skeleton_page(),
            View::Home => self.home_page(cx),
            View::Library => self.library_page(&visible, cx),
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
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .bg(self.appearance.fill(0x081012))
            // Ambient light from the game in view, fading down the window; drawn first so it stays behind.
            .children(tint.map(|color| {
                div().absolute().inset_0().bg(linear_gradient(180., linear_color_stop(rgba(color << 8 | 0x99), 0.), linear_color_stop(rgba(color << 8), 0.9)))
            }))
            .children(backdrop_cover.map(|cover| div().absolute().inset_0().opacity(0.12).child(img(cover).size_full().object_fit(ObjectFit::Cover))))
            .font_family(FONT)
            .text_sm()
            .text_color(rgb(TEXT))
            .child(
                TitleBar::new().bg(self.appearance.fill(0x081012)).child(
                    div().flex_1().flex().items_center().gap_2().child(
                        div()
                            .id("toggle-sidebar")
                            // Keeps the title bar's drag area from taking the click on Windows.
                            .occlude()
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
                    .child(
                        div()
                            .id("rescan")
                            // Keeps the title bar's drag area from taking the click on Windows.
                            .occlude()
                            .w(px(32.))
                            .h(px(32.))
                            .rounded_md()
                            .flex()
                            .items_center()
                            .justify_center()
                            .cursor_pointer()
                            .hover(|style| style.bg(rgba(0xffffff1a)))
                            .child(icon("refresh", 16., rgb(MUTED)))
                            .on_click(cx.listener(|view, _, _, cx| {
                                view.rescan(cx);
                                if !view.games.is_empty() {
                                    view.message = format!("Found {} games.", view.games.len());
                                }
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
            .children(toast)
            .when(self.settings_open && self.details.is_none(), |d| d.child(self.settings_popup(cx)))
            .when(self.search.is_some(), |d| d.child(self.search_popup(cx)))
            .children(self.update_popup(cx))
    }
}

fn main() {
    emu::add_to_app_menu();
    Application::new().with_assets(assets::Assets).run(|cx: &mut App| {
        gpui_component::init(cx);        Theme::change(ThemeMode::Dark, None, cx);
        Theme::global_mut(cx).colors.background = rgba(0x00000000).into();
        let bounds = Bounds::centered(None, size(px(1240.), px(800.)), cx);
        cx.open_window(
            WindowOptions {
                window_background: WindowBackgroundAppearance::Opaque,
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                window_min_size: Some(size(px(860.), px(560.))),
                // Matches emuhub.desktop, so Linux taskbars show the AppImage's icon.
                app_id: Some("emuhub".into()),
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
