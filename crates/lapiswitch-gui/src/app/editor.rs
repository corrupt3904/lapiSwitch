//! DeviceInfo editor shown while the tablet waits in TWRP. Every field of the
//! structure map is editable; inputs are checked for type and exact size.
use super::view::*;
use super::*;
use iced::widget::{
    self, button, checkbox, column, container, row, scrollable, space, text_editor, text_input,
};
use iced::{Alignment, Border, Fill};
use lapiswitch_core::deviceinfo::map::{self, Class, Field, Kind, Layout};
use lapiswitch_core::recovery_flow::EditRequest;
use std::collections::BTreeMap;

/// Basic: the common fields as choices. Custom: every field typed directly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Tab {
    Basic,
    Custom,
}

/// Rollback choice of the basic tab.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RollbackChoice {
    Current,
    Zero,
    Custom,
}

/// Lock fields offered as 0/1 choices on the basic tab.
const LOCK_KEYS: [&str; 2] = ["is_unlocked", "is_unlock_critical"];

fn rollback_key(n: usize) -> String {
    format!("rollback_index[{n}]")
}

/// Fields the basic tab covers; their changes need no extra risk checkbox.
fn is_basic(key: &str) -> bool {
    LOCK_KEYS.contains(&key) || key == "region" || key.starts_with("rollback_index[")
}

#[derive(Debug, Clone)]
pub(crate) enum EditorMessage {
    Input(String, String),
    Reset(String),
    ResetAll,
    Rollback(RollbackChoice),
    Tab(Tab),
    Query(String),
    Review(bool),
    Ack(bool),
    /// Opens or closes one persistent value set on the custom tab.
    Persistent(usize),
    /// Selection in a viewer's value text; edits are ignored.
    Select(String, text_editor::Action),
    Apply,
    Cancel,
}

pub(crate) struct Editor {
    pub(crate) layout: Layout,
    pub(crate) before: Vec<u8>,
    fields: Vec<Field>,
    pub(crate) inputs: BTreeMap<String, String>,
    pub(crate) tab: Tab,
    /// The basic tab shows the 32 rollback inputs.
    pub(crate) rollback_custom: bool,
    pub(crate) query: String,
    pub(crate) review: bool,
    pub(crate) protected_ack: bool,
    pub(crate) firmware_dir: Option<PathBuf>,
    /// The last editor-side check of `firmware_dir` for the planned region.
    pub(crate) firmware: Option<Result<FirmwareReport, String>>,
    pub(crate) checking: bool,
    /// Country code applied after a region change installs firmware;
    /// `None` (the default) keeps the current code.
    pub(crate) country: Option<String>,
    /// Persistent value sets shown open on the custom tab.
    pub(crate) persistent_open: std::collections::BTreeSet<usize>,
    /// Shows every field without inputs (DeviceInfo parsing).
    pub(crate) view_only: bool,
    /// A viewer's full values as selectable text, by field key, and the
    /// rollback indices under `ROLLBACK_TEXT`.
    texts: BTreeMap<String, text_editor::Content>,
    reply: Option<Sender<Option<EditRequest>>>,
}

impl Editor {
    pub(crate) fn new(layout: Layout, before: Vec<u8>, reply: Sender<Option<EditRequest>>) -> Self {
        let fields = layout.fields();
        let inputs = fields
            .iter()
            .map(|field| (field.key.clone(), map::display(field, &before)))
            .collect();
        Self {
            layout,
            before,
            fields,
            inputs,
            tab: Tab::Basic,
            rollback_custom: false,
            query: String::new(),
            review: false,
            protected_ack: false,
            firmware_dir: None,
            firmware: None,
            checking: false,
            country: None,
            persistent_open: Default::default(),
            view_only: false,
            texts: BTreeMap::new(),
            reply: Some(reply),
        }
    }
    /// The fields of a saved DeviceInfo file, shown read-only.
    pub(crate) fn viewer(layout: Layout, bytes: Vec<u8>) -> Self {
        let (reply, _) = std::sync::mpsc::channel();
        let mut viewer = Self::new(layout, bytes, reply);
        viewer.reply = None;
        viewer.view_only = true;
        viewer.tab = Tab::Custom;
        let mut texts: BTreeMap<String, text_editor::Content> = viewer
            .fields
            .iter()
            .map(|field| {
                let value = viewer.original(field);
                (field.key.clone(), text_editor::Content::with_text(&value))
            })
            .collect();
        let rollback = lapiswitch_core::deviceinfo::indices(viewer.layout, &viewer.before)
            .chunks(4)
            .enumerate()
            .map(|(line, values)| {
                values
                    .iter()
                    .enumerate()
                    .map(|(i, value)| format!("[{}] {value}", line * 4 + i))
                    .collect::<Vec<_>>()
                    .join("    ")
            })
            .collect::<Vec<_>>()
            .join("\n");
        texts.insert(
            ROLLBACK_TEXT.into(),
            text_editor::Content::with_text(&rollback),
        );
        viewer.texts = texts;
        viewer
    }
    /// The search and the open persistent sets of a viewer.
    pub(crate) fn view_update(&mut self, message: EditorMessage) {
        match message {
            EditorMessage::Query(query) => self.query = query,
            EditorMessage::Persistent(n) => self.toggle_persistent(n),
            // Selecting and copying only; the value never changes.
            EditorMessage::Select(key, action) if !action.is_edit() => {
                if let Some(content) = self.texts.get_mut(&key) {
                    content.perform(action);
                }
            }
            _ => {}
        }
    }
    fn toggle_persistent(&mut self, n: usize) {
        if !self.persistent_open.remove(&n) {
            self.persistent_open.insert(n);
        }
    }
    fn original(&self, field: &Field) -> String {
        map::display(field, &self.before)
    }
    pub(crate) fn input(&self, field: &Field) -> &str {
        self.inputs.get(&field.key).map_or("", String::as_str)
    }
    pub(crate) fn error(&self, field: &Field) -> Option<String> {
        map::encode(field, self.input(field)).err()
    }
    pub(crate) fn invalid(&self) -> Vec<&Field> {
        self.fields
            .iter()
            .filter(|f| self.error(f).is_some())
            .collect()
    }
    /// The full plan, once every input is valid.
    pub(crate) fn planned(&self) -> Option<Vec<u8>> {
        let edits: map::Edits = self
            .fields
            .iter()
            .filter(|field| self.input(field).trim() != self.original(field))
            .map(|field| (field.key.clone(), self.input(field).to_owned()))
            .collect();
        map::apply(self.layout, &self.before, &edits).ok()
    }
    pub(crate) fn changes(&self) -> Vec<map::Change> {
        self.planned()
            .map(|planned| map::diff(self.layout, &self.before, &planned))
            .unwrap_or_default()
    }
    /// The region the region field asks for. Only that field counts, so an
    /// invalid input elsewhere does not hide the region change.
    pub(crate) fn target_region(&self) -> Option<Region> {
        let field = self.fields.iter().find(|f| f.key == "region")?;
        let edits = map::Edits::from([(field.key.clone(), self.input(field).to_owned())]);
        map::apply(self.layout, &self.before, &edits)
            .ok()
            .and_then(|planned| self.layout.region(&planned))
    }
    pub(crate) fn region_changed(&self) -> bool {
        self.target_region()
            .is_some_and(|target| Some(target) != self.layout.region(&self.before))
    }
    /// Security/structure fields changed outside the basic choices.
    pub(crate) fn protected_changed(&self) -> bool {
        self.changes()
            .iter()
            .any(|change| change.field.class == Class::Protected && !is_basic(&change.field.key))
    }
    fn unchanged(&self, key: &str) -> bool {
        self.fields
            .iter()
            .find(|f| f.key == key)
            .is_none_or(|f| self.input(f).trim() == self.original(f))
    }
    pub(crate) fn rollback_choice(&self) -> RollbackChoice {
        if self.rollback_custom {
            return RollbackChoice::Custom;
        }
        let keys: Vec<String> = (0..map::ROLLBACK_COUNT).map(rollback_key).collect();
        if keys.iter().all(|key| self.unchanged(key)) {
            RollbackChoice::Current
        } else if keys.iter().all(|key| {
            self.fields.iter().find(|f| &f.key == key).is_some_and(|f| {
                map::encode(f, self.input(f)).is_ok_and(|b| b.iter().all(|x| *x == 0))
            })
        }) {
            RollbackChoice::Zero
        } else {
            RollbackChoice::Custom
        }
    }
    fn firmware_ready(&self) -> bool {
        matches!(&self.firmware, Some(Ok(report)) if Some(report.region) == self.target_region())
    }
    /// Why the plan cannot be applied yet.
    pub(crate) fn blocker(&self, app: &App) -> Option<&'static str> {
        if !self.invalid().is_empty() {
            Some(app.tr("editor_some_fields_have_invalid_input"))
        } else if self.changes().is_empty() {
            Some(app.tr("editor_no_field_has_been_changed"))
        } else if self.target_region().is_none() {
            Some(app.tr("editor_the_region_field_must_be_prc"))
        } else if self.region_changed() && !self.firmware_ready() {
            Some(app.tr("editor_changing_the_region_requires_a_checked"))
        } else if self.protected_changed() && !self.protected_ack {
            Some(app.tr("editor_confirm_the_risk_of_changing_security"))
        } else {
            None
        }
    }
    fn send(&mut self, request: Option<EditRequest>) {
        if let Some(reply) = self.reply.take() {
            let _ = reply.send(request);
        }
    }
    /// Firmware and folder only matter for a region change.
    pub(crate) fn request(&self) -> Option<EditRequest> {
        let planned = self.planned()?;
        let flash = self.region_changed();
        Some(EditRequest {
            edits: map::edits_between(self.layout, &self.before, &planned),
            firmware_dir: flash.then(|| self.firmware_dir.clone()).flatten(),
            country: flash.then(|| self.country.clone()).flatten(),
        })
    }
}

impl App {
    pub(super) fn editor_update(&mut self, message: EditorMessage) {
        let Some(ready) = self.editor.as_ref().map(|e| e.blocker(self).is_none()) else {
            return;
        };
        let editor = self.editor.as_mut().expect("checked");
        let firmware_target = |editor: &Editor| editor.target_region();
        let before_target = firmware_target(editor);
        match message {
            EditorMessage::Input(key, value) => {
                // The region is typed as the token it is written as, and a
                // digit-limited field takes no more than its digits.
                let limit = editor
                    .fields
                    .iter()
                    .find(|f| f.key == key)
                    .map_or(0, |f| f.max_digits);
                let value = if key == "region" {
                    value.to_ascii_uppercase()
                } else if limit > 0 {
                    value.chars().take(limit).collect()
                } else {
                    value
                };
                editor.inputs.insert(key, value);
            }
            EditorMessage::Reset(key) => {
                if let Some(field) = editor.fields.iter().find(|f| f.key == key).cloned() {
                    let original = editor.original(&field);
                    editor.inputs.insert(key, original);
                }
            }
            EditorMessage::ResetAll => {
                for field in editor.fields.clone() {
                    let original = editor.original(&field);
                    editor.inputs.insert(field.key, original);
                }
                editor.protected_ack = false;
                editor.rollback_custom = false;
            }
            EditorMessage::Rollback(choice) => {
                editor.rollback_custom = choice == RollbackChoice::Custom;
                for n in 0..map::ROLLBACK_COUNT {
                    let key = rollback_key(n);
                    match choice {
                        RollbackChoice::Current => {
                            if let Some(field) =
                                editor.fields.iter().find(|f| f.key == key).cloned()
                            {
                                let original = editor.original(&field);
                                editor.inputs.insert(key, original);
                            }
                        }
                        RollbackChoice::Zero => {
                            editor.inputs.insert(key, "0".into());
                        }
                        RollbackChoice::Custom => {}
                    }
                }
            }
            EditorMessage::Tab(tab) => editor.tab = tab,
            EditorMessage::Query(query) => editor.query = query,
            EditorMessage::Review(review) => {
                editor.review = review && ready;
                return;
            }
            EditorMessage::Ack(ack) => editor.protected_ack = ack,
            EditorMessage::Persistent(n) => editor.toggle_persistent(n),
            EditorMessage::Select(..) => return,
            EditorMessage::Apply => {
                if editor.review && ready {
                    let request = editor.request();
                    editor.send(request);
                    self.editor = None;
                }
                return;
            }
            EditorMessage::Cancel => {
                editor.send(None);
                self.editor = None;
                return;
            }
        }
        // A different planned region invalidates an earlier firmware check.
        let editor = self.editor.as_mut().expect("still open");
        if firmware_target(editor) != before_target {
            editor.firmware = None;
            editor.checking = false;
            if let (Some(dir), Some(region)) = (editor.firmware_dir.clone(), editor.target_region())
                && editor.region_changed()
            {
                editor.checking = true;
                worker::validate_editor_firmware(self.tx.clone(), dir, region);
            }
        }
    }
    /// The country picker while the editor is open: it edits the editor's
    /// code, not the conversion option.
    pub(super) fn editor_country(&mut self, message: Message) {
        let Some(editor) = self.editor.as_mut() else {
            return;
        };
        match message {
            Message::CountryOpen if editor.region_changed() => {
                self.country_open = true;
                self.country_search.clear();
                self.country_draft = Some(editor.country.clone().unwrap_or_else(|| "—".into()));
            }
            Message::CountrySearch(query) if self.country_open => self.country_search = query,
            Message::Country(code)
                if self.country_open
                    && (code == "—" || crate::countries::find(&code).is_some()) =>
            {
                self.country_draft = Some(code);
            }
            Message::CountryCancel => {
                self.country_open = false;
                self.country_draft = None;
            }
            Message::CountryApply if self.country_open => {
                if let Some(code) = self.country_draft.take() {
                    editor.country = (code != "—").then_some(code);
                }
                self.country_open = false;
            }
            _ => {}
        }
    }
    pub(super) fn editor_firmware_picked(&mut self, folder: Option<PathBuf>) {
        let Some(editor) = self.editor.as_mut() else {
            return;
        };
        let Some(folder) = folder else {
            return;
        };
        editor.firmware_dir = Some(folder.clone());
        editor.firmware = None;
        editor.checking = false;
        if let Some(region) = editor.target_region()
            && editor.region_changed()
        {
            editor.checking = true;
            worker::validate_editor_firmware(self.tx.clone(), folder, region);
        }
    }
    pub(super) fn editor_firmware_checked(&mut self, result: Result<FirmwareReport, String>) {
        if let Some(editor) = self.editor.as_mut() {
            editor.checking = false;
            editor.firmware = Some(result);
        }
    }
}

fn tab_label(app: &App, tab: Tab) -> &'static str {
    match tab {
        Tab::Basic => app.tr("editor_basic"),
        Tab::Custom => app.tr("editor_custom"),
    }
}

/// One selectable option; the chosen one is highlighted.
fn choice<'a>(text: String, selected: bool, message: EditorMessage) -> Element<'a, Message> {
    button(label(text).size(14).color(if selected {
        super::fluent::p().on_accent
    } else {
        ink()
    }))
    .padding([7, 14])
    .style(if selected {
        super::fluent::accent_style
    } else {
        soft_style
    })
    .on_press(Message::Editor(message))
    .into()
}

/// The values a field may be set to by button; empty for a typed field.
fn options(field: &Field) -> &'static [&'static str] {
    if field.kind == Kind::Region {
        &["PRC", "ROW"]
    } else {
        field.choices
    }
}

/// A basic-tab field by key.
fn basic_choice<'a>(app: &'a App, editor: &'a Editor, key: &str) -> Element<'a, Message> {
    match editor.fields.iter().find(|f| f.key == key) {
        Some(field) => choice_row(app, editor, field, true),
        None => space().into(),
    }
}

/// A field set by buttons: its current value and the other options. A read
/// value that is none of the options can only be kept.
fn choice_row<'a>(
    app: &'a App,
    editor: &'a Editor,
    field: &'a Field,
    basic: bool,
) -> Element<'a, Message> {
    let options = options(field);
    let original = editor.original(field);
    let value = editor.input(field).trim();
    let unchanged = editor.unchanged(&field.key);
    let locked = if field.kind == Kind::Region {
        editor.layout.region(&editor.before).is_none()
    } else {
        !options.contains(&original.as_str())
    };
    let mut choices = row![choice(
        format!("{} ({})", app.tr("editor_current"), short(&original)),
        unchanged,
        EditorMessage::Reset(field.key.clone()),
    )]
    .spacing(8)
    .align_y(Alignment::Center);
    if !locked {
        // An option equal to the current value would duplicate "Current".
        for option in options.iter().filter(|option| original != **option) {
            choices = choices.push(choice(
                (*option).into(),
                !unchanged && value == *option,
                EditorMessage::Input(field.key.clone(), (*option).into()),
            ));
        }
    }
    let description = app
        .preferences
        .language
        .field_description(field.description);
    let place = format!("0x{:04X} · {}B", field.offset, field.size);
    let head = if basic {
        row![
            label(description).size(15),
            small(format!("{} · {place}", field.key)),
        ]
    } else {
        row![
            label(field.key.clone()).size(15),
            small(place),
            small(description),
        ]
    }
    .spacing(10)
    .align_y(Alignment::Center);
    let mut content = column![head, choices].spacing(8);
    if locked {
        content = content.push(small(app.tr("editor_the_read_value_is_not_one_of")));
    }
    basic_box(content.extend(field_notice(app, editor, field)), !unchanged)
}

/// A read-only field: its value without an input.
fn read_only_row<'a>(app: &'a App, editor: &'a Editor, field: &'a Field) -> Element<'a, Message> {
    let mut head = row![
        label(field.key.clone()).size(15),
        small(format!("0x{:04X} · {}B", field.offset, field.size)),
        small(
            app.preferences
                .language
                .field_description(field.description)
        ),
    ]
    .spacing(10)
    .align_y(Alignment::Center);
    // A viewer is read-only as a whole, so the mark is left out there.
    if !field.editable && !editor.view_only {
        head = head
            .push(space().width(Fill))
            .push(small(app.tr("editor_read_only")));
    }
    if let Some(value) = selectable(editor, &field.key) {
        return basic_box(column![head, value].spacing(6), false);
    }
    basic_box(
        column![head, label(short(&editor.original(field))).size(14),].spacing(6),
        false,
    )
}

/// The notice a changed field shows inside its own box: the region change
/// with its firmware folder and country code, the lock state warning, or the
/// security/structure warning of other protected fields.
fn field_notice<'a>(
    app: &'a App,
    editor: &'a Editor,
    field: &Field,
) -> Option<Element<'a, Message>> {
    if editor.unchanged(&field.key) {
        return None;
    }
    if field.key == "region" {
        region_box(app, editor)
    } else if LOCK_KEYS.contains(&field.key.as_str()) {
        Some(note(
            app.tr("editor_lock_state_values_affect_bootloader_behaviour"),
            true,
        ))
    } else if field.class == Class::Protected && !is_basic(&field.key) {
        Some(note(
            app.tr("editor_this_security_structure_field_can_stop"),
            true,
        ))
    } else {
        None
    }
}

/// Changed fields get a red outline so edits stand out.
fn changed_border(changed: bool) -> Border {
    Border {
        color: if changed { danger() } else { line() },
        width: if changed { 2.0 } else { 1.0 },
        radius: super::fluent::CARD_RADIUS.into(),
    }
}

fn basic_box<'a>(content: widget::Column<'a, Message>, changed: bool) -> Element<'a, Message> {
    container(content)
        .padding(12)
        .width(Fill)
        .style(move |_| container::Style {
            background: Some(if changed { pale() } else { card_color() }.into()),
            border: changed_border(changed),
            ..Default::default()
        })
        .into()
}

fn rollback_row<'a>(app: &'a App, editor: &'a Editor) -> Element<'a, Message> {
    if editor.view_only {
        return rollback_values(app, editor);
    }
    let selected = editor.rollback_choice();
    // "0" duplicates "Current" when every stored index is already 0.
    let all_zero = lapiswitch_core::deviceinfo::indices(editor.layout, &editor.before)
        .iter()
        .all(|index| *index == 0);
    let mut choices = row![choice(
        if all_zero {
            format!("{} (0)", app.tr("editor_current"))
        } else {
            app.tr("editor_current").into()
        },
        selected == RollbackChoice::Current,
        EditorMessage::Rollback(RollbackChoice::Current),
    )]
    .spacing(8);
    if !all_zero {
        choices = choices.push(choice(
            "0".into(),
            selected == RollbackChoice::Zero,
            EditorMessage::Rollback(RollbackChoice::Zero),
        ));
    }
    choices = choices.push(choice(
        app.tr("editor_custom").into(),
        selected == RollbackChoice::Custom,
        EditorMessage::Rollback(RollbackChoice::Custom),
    ));
    let mut content = column![
        row![
            label(app.tr("editor_32_rollback_indices")).size(15),
            small(format!(
                "rollback_index[0..31] · 0x{:04X} · 8B × 32",
                editor.layout.rollback_start()
            )),
        ]
        .spacing(10)
        .align_y(Alignment::Center),
        choices,
    ]
    .spacing(8);
    if selected == RollbackChoice::Custom {
        for n in 0..map::ROLLBACK_COUNT {
            if let Some(field) = editor.fields.iter().find(|f| f.key == rollback_key(n)) {
                content = content.push(field_row(app, editor, field));
            }
        }
    }
    basic_box(content, selected != RollbackChoice::Current)
}

/// The key of a viewer's rollback index text.
const ROLLBACK_TEXT: &str = "rollback_index[0..31]";

/// A viewer's full value as text that can be selected and copied.
fn selectable<'a>(editor: &'a Editor, key: &str) -> Option<Element<'a, Message>> {
    let content = editor.texts.get(key)?;
    let key = key.to_owned();
    Some(
        text_editor(content)
            .on_action(move |action| Message::Editor(EditorMessage::Select(key.clone(), action)))
            .size(14)
            .wrapping(iced::widget::text::Wrapping::Glyph)
            .into(),
    )
}

/// The 32 rollback indices as values, for a viewer.
fn rollback_values<'a>(app: &'a App, editor: &'a Editor) -> Element<'a, Message> {
    let grid = selectable(editor, ROLLBACK_TEXT).unwrap_or_else(|| space().into());
    basic_box(
        column![
            row![
                label(app.tr("editor_32_rollback_indices")).size(15),
                small(format!(
                    "rollback_index[0..31] · 0x{:04X} · 8B × 32",
                    editor.layout.rollback_start()
                )),
            ]
            .spacing(10)
            .align_y(Alignment::Center),
            grid,
        ]
        .spacing(8),
        false,
    )
}

fn kind_hint(app: &App, field: &Field) -> String {
    match field.kind {
        Kind::Flag => app.tr("editor_0_or_1").into(),
        Kind::Uint if field.max_digits > 0 => app
            .tr("editor_decimal_up_to_digits")
            .replace("{digits}", &field.max_digits.to_string()),
        Kind::Uint if field.decimal => app
            .tr("editor_bytes_byte_integer_decimal")
            .replace("{bytes}", &field.size.to_string()),
        Kind::Uint => app
            .tr("editor_bytes_byte_integer_decimal_or_0x")
            .replace("{bytes}", &field.size.to_string()),
        Kind::Region => "PRC / ROW".into(),
        Kind::Text { nul } => app
            .tr("editor_ascii_up_to_chars_characters")
            .replace("{chars}", &(field.size - usize::from(nul)).to_string()),
        Kind::Bytes => app
            .tr("editor_bytes_bytes_of_hex")
            .replace("{bytes}", &field.size.to_string()),
    }
}

/// Long values (keys, caches) are summarized in the review list.
fn short(value: &str) -> String {
    if value.chars().count() > 48 {
        let head: String = value.chars().take(40).collect();
        lapiswitch_core::msg!(
            "run_text_truncated",
            head = head,
            count = value.chars().count()
        )
    } else {
        value.to_owned()
    }
}

fn field_row<'a>(app: &'a App, editor: &'a Editor, field: &'a Field) -> Element<'a, Message> {
    if !field.editable || editor.view_only {
        return read_only_row(app, editor, field);
    }
    if !options(field).is_empty() {
        return choice_row(app, editor, field, false);
    }
    let value = editor.input(field);
    let changed = value.trim() != editor.original(field);
    let mut head = row![
        label(field.key.clone()).size(15),
        small(format!("0x{:04X} · {}B", field.offset, field.size)),
        small(
            app.preferences
                .language
                .field_description(field.description)
        ),
    ]
    .spacing(10)
    .align_y(Alignment::Center);
    if changed {
        head = head.push(space().width(Fill)).push(
            button(small(app.tr("editor_original")))
                .padding([4, 10])
                .style(soft_style)
                .on_press(Message::Editor(EditorMessage::Reset(field.key.clone()))),
        );
    }
    let key = field.key.clone();
    let mut content = column![
        head,
        text_input(&kind_hint(app, field), value)
            .on_input(move |text| Message::Editor(EditorMessage::Input(key.clone(), text)))
            .padding(8)
            .size(14),
    ]
    .spacing(6);
    if let Some(error) = editor.error(field) {
        content = content.push(small(error).color(danger()));
    } else if changed {
        content = content.push(small(format!(
            "{}: {}",
            app.tr("editor_original"),
            short(&editor.original(field))
        )));
    }
    let content = content.extend(field_notice(app, editor, field));
    container(content)
        .padding(12)
        .width(Fill)
        .style(move |_| container::Style {
            background: Some(if changed { pale() } else { card_color() }.into()),
            border: changed_border(changed),
            ..Default::default()
        })
        .into()
}

/// The set number of a `persistent[n].…` key.
fn persistent_index(key: &str) -> Option<usize> {
    key.strip_prefix("persistent[")?
        .split_once(']')?
        .0
        .parse()
        .ok()
}

/// The 8 persistent value sets as one box: a summary line per set that
/// opens its 5 fields.
fn persistent_box<'a>(app: &'a App, editor: &'a Editor) -> Element<'a, Message> {
    let sets: Vec<usize> = editor
        .fields
        .iter()
        .filter_map(|f| persistent_index(&f.key))
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();
    let set_fields = |n: usize| {
        editor
            .fields
            .iter()
            .filter(move |f| persistent_index(&f.key) == Some(n))
    };
    let changed_in = |n: usize| set_fields(n).filter(|f| !editor.unchanged(&f.key)).count();
    let changed: usize = sets.iter().map(|n| changed_in(*n)).sum();
    let value = |n: usize, part: &str| {
        editor
            .fields
            .iter()
            .find(|f| f.key == format!("persistent[{n}].{part}"))
            .map(|f| editor.input(f).to_owned())
            .unwrap_or_default()
    };
    let mut title = app
        .tr("editor_avb_persistent_values_count")
        .replace("{count}", &sets.len().to_string());
    if changed > 0 {
        title.push(' ');
        title.push_str(
            &app.tr("editor_count_changed")
                .replace("{count}", &changed.to_string()),
        );
    }
    let mut content = column![
        row![
            label(title).size(15),
            small(format!("persistent[0..{}]", sets.len().saturating_sub(1))),
        ]
        .spacing(10)
        .align_y(Alignment::Center)
    ]
    .spacing(8);
    for n in sets {
        let open = editor.persistent_open.contains(&n) || changed_in(n) > 0;
        let used = value(n, "valid") != "0";
        let name = value(n, "name");
        let summary = format!(
            "{}  {}",
            if open { "▾" } else { "▸" },
            app.tr("editor_persistent_summary")
                .replace("{n}", &n.to_string())
                .replace(
                    "{state}",
                    if used {
                        app.tr("editor_in_use")
                    } else {
                        app.tr("editor_not_in_use")
                    }
                )
                .replace(
                    "{name}",
                    &if name.is_empty() {
                        app.tr("editor_none").to_owned()
                    } else {
                        format!("\"{}\"", short(&name))
                    }
                )
                .replace("{size}", &value(n, "value_length")),
        );
        content = content.push(
            button(label(summary).size(14).color(ink()))
                .padding([8, 12])
                .width(Fill)
                .style(soft_style)
                .on_press(Message::Editor(EditorMessage::Persistent(n))),
        );
        if open {
            let mut fields = column![].spacing(8).padding([0, 0]);
            for field in set_fields(n) {
                fields = fields.push(field_row(app, editor, field));
            }
            content = content.push(container(fields).padding(iced::Padding {
                left: 20.0,
                ..Default::default()
            }));
        }
    }
    basic_box(content, changed > 0)
}

fn region_box<'a>(app: &'a App, editor: &'a Editor) -> Option<Element<'a, Message>> {
    if !editor.region_changed() {
        return None;
    }
    let from = editor
        .layout
        .region(&editor.before)
        .map_or("?", Region::token);
    let to = editor.target_region().map_or("?", Region::token);
    let status: String = match (&editor.firmware, editor.checking) {
        (_, true) => app.tr("editor_checking_firmware").into(),
        (Some(Ok(report)), _) => {
            format!("{} · {}", app.tr("editor_checked"), report.fingerprint)
        }
        (Some(Err(error)), _) => error.clone(),
        (None, _) => app.tr("editor_choose_a_firmware_folder").into(),
    };
    Some(
        column![
            note(
                app.tr("editor_region_from_to_the_official_firmware")
                    .replace("{from}", from)
                    .replace("{to}", to),
                true
            ),
            row![
                action(
                    app.tr("editor_choose_firmware_folder"),
                    Message::Pick(Picker::EditorFirmware),
                    !editor.checking
                )
                .style(soft_style),
                small(status).width(Fill),
            ]
            .spacing(12)
            .align_y(Alignment::Center),
            row![
                small(app.tr("editor_country_code")),
                label(country_label(app, editor)).width(Fill),
                action(app.tr("editor_choose"), Message::CountryOpen, true).style(soft_style),
            ]
            .spacing(12)
            .align_y(Alignment::Center),
        ]
        .spacing(10)
        .into(),
    )
}

/// The country code a region change applies after installing firmware.
fn country_label(app: &App, editor: &Editor) -> String {
    match editor.country.as_deref().and_then(crate::countries::find) {
        Some(country) => format!("{} · {}", country.code, country.name),
        None => app.tr("editor_do_not_change_default").into(),
    }
}

fn review<'a>(app: &'a App, editor: &'a Editor) -> widget::Container<'a, Message> {
    let changes = editor.changes();
    let mut list = column![].spacing(6);
    for change in &changes {
        let risky = change.field.class == Class::Protected;
        list = list.push(
            column![
                label(format!(
                    "{} · 0x{:04X} · {}B",
                    change.field.key, change.field.offset, change.field.size
                ))
                .size(14)
                .color(if risky { danger() } else { ink() }),
                small(format!(
                    "{} → {}",
                    short(&change.before),
                    short(&change.after)
                )),
            ]
            .spacing(2),
        );
    }
    let flash = editor.region_changed();
    panel(
        column![
            label(app.tr("editor_review_changes")).size(25),
            small(
                app.tr("editor_count_fields_change")
                    .replace("{count}", &changes.len().to_string())
            ),
            scrollable(list)
                .direction(super::fluent::thin_scrollbar())
                .style(super::fluent::scroll_style)
                .height(if flash { 340 } else { 380 }),
            if flash {
                row![
                    small(app.tr("editor_country_code")),
                    label(country_label(app, editor)),
                ]
                .spacing(12)
            } else {
                row![]
            },
            note(
                if flash {
                    app.tr("editor_applying_writes_and_verifies_deviceinfo_restores")
                } else {
                    app.tr("editor_applying_writes_and_verifies_deviceinfo_restores_2")
                },
                flash
            ),
            row![
                space().width(Fill),
                action(
                    app.tr("editor_back"),
                    Message::Editor(EditorMessage::Review(false)),
                    true
                )
                .style(soft_style),
                action(
                    app.tr("editor_apply"),
                    Message::Editor(EditorMessage::Apply),
                    true
                )
                .style(if editor.protected_changed() || flash {
                    super::fluent::danger_style
                } else {
                    super::fluent::accent_style
                }),
            ]
            .spacing(12),
        ]
        .spacing(16),
    )
    .max_width(820)
}

/// Every field of the custom tab, filtered by the search; without a search
/// the rollback indices and the persistent value sets are one box each.
pub(super) fn field_list<'a>(app: &'a App, editor: &'a Editor) -> widget::Column<'a, Message> {
    let mut list = column![].spacing(8);
    // The key, the English description and the shown translation all match.
    let query = editor.query.trim().to_lowercase();
    let language = app.preferences.language;
    // Without a search the rollback indices and the persistent value sets
    // are one grouped box each.
    let grouped = query.is_empty();
    let mut rollback_shown = false;
    let mut persistent_shown = false;
    for field in editor.fields.iter().filter(|field| {
        query.is_empty()
            || field.key.to_lowercase().contains(&query)
            || field.description.to_lowercase().contains(&query)
            || language
                .field_description(field.description)
                .to_lowercase()
                .contains(&query)
    }) {
        if grouped && field.key.starts_with("rollback_index[") {
            if !rollback_shown {
                rollback_shown = true;
                list = list.push(rollback_row(app, editor));
            }
            continue;
        }
        if grouped && persistent_index(&field.key).is_some() {
            if !persistent_shown {
                persistent_shown = true;
                list = list.push(persistent_box(app, editor));
            }
            continue;
        }
        list = list.push(field_row(app, editor, field));
    }
    list
}

pub(super) fn modal<'a>(app: &'a App, editor: &'a Editor) -> widget::Container<'a, Message> {
    if editor.review {
        return review(app, editor);
    }
    let changes = editor.changes();
    let mut tabs = row![].spacing(8);
    for tab in [Tab::Basic, Tab::Custom] {
        let changed = changes
            .iter()
            .filter(|change| tab == Tab::Custom || is_basic(&change.field.key))
            .count();
        let title = if changed > 0 {
            format!("{} ({changed})", tab_label(app, tab))
        } else {
            tab_label(app, tab).to_owned()
        };
        tabs = tabs.push(choice(title, editor.tab == tab, EditorMessage::Tab(tab)));
    }
    let mut list = column![].spacing(8);
    if editor.tab == Tab::Basic {
        list = list
            .push(basic_choice(app, editor, "region"))
            .push(basic_choice(app, editor, LOCK_KEYS[0]))
            .push(basic_choice(app, editor, LOCK_KEYS[1]))
            .push(rollback_row(app, editor));
    } else {
        list = list
            .push(note(
                app.tr("editor_every_field_is_typed_directly_wrong"),
                true,
            ))
            .push(field_list(app, editor));
    }
    let mut body = column![
        label(
            app.tr("editor_edit_deviceinfo_model")
                .replace("{model}", editor.layout.model())
        )
        .size(25),
        small(
            app.tr("editor_current_values_read_in_twrp_change")
                .replace("{size}", &format!("{:X}", editor.layout.logical_size()))
        ),
        if editor.tab == Tab::Custom {
            row![
                tabs,
                space().width(Fill),
                text_input(app.tr("editor_search_fields"), &editor.query)
                    .on_input(|text| Message::Editor(EditorMessage::Query(text)))
                    .padding(8)
                    .width(220),
            ]
        } else {
            row![tabs]
        }
        .spacing(12)
        .align_y(Alignment::Center),
        scrollable(list)
            .direction(super::fluent::thin_scrollbar())
            .style(super::fluent::scroll_style)
            .height(400),
    ]
    .spacing(14);
    if editor.protected_changed() {
        body = body.push(
            checkbox(editor.protected_ack)
                .label(app.tr("editor_i_understand_that_changing_security_structure"))
                .on_toggle(|ack| Message::Editor(EditorMessage::Ack(ack))),
        );
    }
    let blocker = editor.blocker(app);
    body = body.push(
        row![
            small(blocker.unwrap_or("")).width(Fill),
            action(
                app.tr("editor_reset_all"),
                Message::Editor(EditorMessage::ResetAll),
                true
            )
            .style(soft_style),
            action(
                app.tr("editor_cancel_restore_originals"),
                Message::Editor(EditorMessage::Cancel),
                true
            )
            .style(soft_style),
            action(
                app.tr("editor_review_changes"),
                Message::Editor(EditorMessage::Review(true)),
                blocker.is_none()
            ),
        ]
        .spacing(12)
        .align_y(Alignment::Center),
    );
    panel(body).max_width(980)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layout() -> Layout {
        lapiswitch_core::device::Device::find("TB520FU")
            .unwrap()
            .layout()
            .unwrap()
    }
    fn open() -> (App, std::sync::mpsc::Receiver<Option<EditRequest>>) {
        let mut app = App::base(true);
        let (tx, rx) = std::sync::mpsc::channel();
        app.editor = Some(Editor::new(
            layout(),
            super::super::demo_deviceinfo(layout()),
            tx,
        ));
        (app, rx)
    }
    fn editor(app: &App) -> &Editor {
        app.editor.as_ref().unwrap()
    }

    #[test]
    fn basic_choices_set_values_and_current_restores_the_read_value() {
        let (mut app, _rx) = open();
        assert_eq!(editor(&app).tab, Tab::Basic);
        assert_eq!(editor(&app).rollback_choice(), RollbackChoice::Current);
        app.editor_update(EditorMessage::Rollback(RollbackChoice::Zero));
        assert_eq!(editor(&app).rollback_choice(), RollbackChoice::Zero);
        let planned = editor(&app).planned().unwrap();
        assert_eq!(
            lapiswitch_core::deviceinfo::indices(layout(), &planned),
            [0; 32]
        );
        app.editor_update(EditorMessage::Rollback(RollbackChoice::Current));
        assert!(editor(&app).changes().is_empty());
        // Custom keeps the values and opens the 32 inputs.
        app.editor_update(EditorMessage::Rollback(RollbackChoice::Custom));
        assert_eq!(editor(&app).rollback_choice(), RollbackChoice::Custom);
        assert!(editor(&app).changes().is_empty());
        app.editor_update(EditorMessage::Input(rollback_key(3), "20".into()));
        let planned = editor(&app).planned().unwrap();
        assert_eq!(
            lapiswitch_core::deviceinfo::indices(layout(), &planned)[3],
            20
        );
        app.editor_update(EditorMessage::ResetAll);
        assert_eq!(editor(&app).rollback_choice(), RollbackChoice::Current);
    }

    #[test]
    fn basic_lock_changes_need_no_risk_checkbox_but_custom_security_fields_do() {
        let (mut app, _rx) = open();
        app.editor_update(EditorMessage::Input("is_unlocked".into(), "1".into()));
        app.editor_update(EditorMessage::Input(
            "is_unlock_critical".into(),
            "1".into(),
        ));
        assert!(!editor(&app).protected_changed());
        assert!(editor(&app).blocker(&app).is_none());
        app.editor_update(EditorMessage::Input("verity_mode".into(), "1".into()));
        assert!(editor(&app).protected_changed());
        assert!(editor(&app).blocker(&app).is_some());
        app.editor_update(EditorMessage::Ack(true));
        assert!(editor(&app).blocker(&app).is_none());
    }

    #[test]
    fn region_choice_requires_firmware_and_differing_custom_rollbacks_show_custom() {
        let (mut app, _rx) = open();
        app.editor_update(EditorMessage::Input("region".into(), "ROW".into()));
        assert!(editor(&app).region_changed());
        assert!(editor(&app).blocker(&app).is_some());
        app.editor_update(EditorMessage::Reset("region".into()));
        assert!(!editor(&app).region_changed());
        // A value typed on the custom tab is reflected on the basic tab.
        app.editor_update(EditorMessage::Tab(Tab::Custom));
        app.editor_update(EditorMessage::Input(rollback_key(7), "5".into()));
        assert_eq!(editor(&app).rollback_choice(), RollbackChoice::Custom);
    }

    #[test]
    fn rollback_indices_take_at_most_ten_decimal_digits() {
        let (mut app, _rx) = open();
        app.editor_update(EditorMessage::Input(rollback_key(4), "123456789012".into()));
        let field = editor(&app)
            .fields
            .iter()
            .find(|f| f.key == rollback_key(4))
            .unwrap();
        assert_eq!(editor(&app).input(field), "1234567890");
        assert!(editor(&app).error(field).is_none());
        app.editor_update(EditorMessage::Input(rollback_key(4), "0x10".into()));
        let field = editor(&app)
            .fields
            .iter()
            .find(|f| f.key == rollback_key(4))
            .unwrap();
        assert!(editor(&app).error(field).is_some());
    }

    #[test]
    fn the_region_is_typed_in_capitals() {
        let (mut app, _rx) = open();
        app.editor_update(EditorMessage::Input("region".into(), "prc".into()));
        let field = editor(&app)
            .fields
            .iter()
            .find(|f| f.key == "region")
            .unwrap();
        assert_eq!(editor(&app).input(field), "PRC");
        assert!(editor(&app).unchanged("region"));
        app.editor_update(EditorMessage::Input("region".into(), "row".into()));
        assert!(editor(&app).region_changed());
    }

    #[test]
    fn an_invalid_other_field_keeps_the_region_change_and_its_firmware_check() {
        let (mut app, _rx) = open();
        app.editor_update(EditorMessage::Input("region".into(), "ROW".into()));
        app.demo_pick(Picker::EditorFirmware);
        assert!(editor(&app).firmware_ready());
        for value in ["", "x"] {
            app.editor_update(EditorMessage::Input("is_unlocked".into(), value.into()));
            assert!(editor(&app).region_changed());
            assert_eq!(editor(&app).target_region(), Some(Region::Row));
            assert!(editor(&app).firmware_dir.is_some());
            assert!(editor(&app).firmware_ready());
            assert_eq!(
                editor(&app).blocker(&app),
                Some(app.tr("editor_some_fields_have_invalid_input"))
            );
        }
        app.editor_update(EditorMessage::Input("is_unlocked".into(), "1".into()));
        assert!(editor(&app).blocker(&app).is_none());
    }

    #[test]
    fn the_country_code_is_offered_only_for_a_region_change_and_defaults_to_unchanged() {
        let (mut app, _rx) = open();
        // Without a region change the picker does not open.
        let _ = app.update(Message::CountryOpen);
        assert!(!app.country_open);
        app.editor_update(EditorMessage::Input("region".into(), "ROW".into()));
        assert_eq!(editor(&app).request().unwrap().country, None);
        let _ = app.update(Message::CountryOpen);
        assert!(app.country_open);
        assert_eq!(app.country_draft.as_deref(), Some("—"));
        let _ = app.update(Message::Country("KR".into()));
        let _ = app.update(Message::CountryApply);
        assert!(!app.country_open);
        assert_eq!(editor(&app).country.as_deref(), Some("KR"));
        assert_eq!(
            editor(&app).request().unwrap().country.as_deref(),
            Some("KR")
        );
        // The conversion option is untouched.
        assert_eq!(app.country, None);
        // "Do not change" clears it again.
        let _ = app.update(Message::CountryOpen);
        let _ = app.update(Message::Country("—".into()));
        let _ = app.update(Message::CountryApply);
        assert_eq!(editor(&app).country, None);
        // Reverting the region drops any chosen code from the request.
        let _ = app.update(Message::CountryOpen);
        let _ = app.update(Message::Country("JP".into()));
        let _ = app.update(Message::CountryApply);
        app.editor_update(EditorMessage::Reset("region".into()));
        app.editor_update(EditorMessage::Input("rollback_index[1]".into(), "3".into()));
        assert_eq!(editor(&app).request().unwrap().country, None);
    }
}
