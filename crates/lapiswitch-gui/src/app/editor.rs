//! DeviceInfo editor shown while the tablet waits in TWRP. Every field of the
//! structure map is editable; inputs are checked for type and exact size.
use super::view::*;
use super::*;
use iced::widget::{self, button, checkbox, column, container, row, scrollable, space, text_input};
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
            reply: Some(reply),
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
    pub(crate) fn target_region(&self) -> Option<Region> {
        self.planned()
            .and_then(|planned| self.layout.region(&planned))
    }
    pub(crate) fn region_changed(&self) -> bool {
        self.planned()
            .is_some_and(|planned| self.layout.region(&planned) != self.layout.region(&self.before))
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
            Some(app.t(
                "입력 형식이 맞지 않는 필드가 있습니다.",
                "Some fields have invalid input.",
            ))
        } else if self.changes().is_empty() {
            Some(app.t("변경된 필드가 없습니다.", "No field has been changed."))
        } else if self.target_region().is_none() {
            Some(app.t(
                "리전 필드는 PRC 또는 ROW여야 합니다.",
                "The region field must be PRC or ROW.",
            ))
        } else if self.region_changed() && !self.firmware_ready() {
            Some(app.t(
                "리전을 바꾸려면 해당 리전의 펌웨어 폴더를 선택해 검사를 마쳐야 합니다.",
                "Changing the region requires a checked firmware folder for that region.",
            ))
        } else if self.protected_changed() && !self.protected_ack {
            Some(app.t(
                "보안·구조 필드 변경 위험 확인에 체크해 주세요.",
                "Confirm the risk of changing security/structure fields.",
            ))
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
        Tab::Basic => app.t("기본", "Basic"),
        Tab::Custom => app.t("사용자지정", "Custom"),
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

/// A basic-tab field: its current value and fixed options. A value set on
/// the custom tab that is none of the options is shown as such.
fn choice_row<'a>(
    app: &'a App,
    editor: &'a Editor,
    key: &'static str,
    options: &[&'static str],
) -> Element<'a, Message> {
    let Some(field) = editor.fields.iter().find(|f| f.key == key) else {
        return space().into();
    };
    let original = editor.original(field);
    let value = editor.input(field).trim().to_ascii_uppercase();
    let unchanged = editor.unchanged(key);
    let mut choices = row![choice(
        format!("{} ({})", app.t("현재값", "Current"), short(&original)),
        unchanged,
        EditorMessage::Reset(key.into()),
    )]
    .spacing(8)
    .align_y(Alignment::Center);
    // An option equal to the current value would duplicate "Current".
    for option in options
        .iter()
        .filter(|option| !original.eq_ignore_ascii_case(option))
    {
        choices = choices.push(choice(
            (*option).into(),
            !unchanged && value == *option,
            EditorMessage::Input(key.into(), (*option).into()),
        ));
    }
    if !unchanged && !options.contains(&value.as_str()) {
        choices = choices.push(small(format!(
            "{}: {}",
            app.t("사용자지정 값", "Custom value"),
            short(editor.input(field))
        )));
    }
    basic_box(
        column![
            row![
                label(
                    app.preferences
                        .language
                        .field_description(field.description)
                )
                .size(15),
                small(format!(
                    "{} · 0x{:04X} · {}B",
                    field.key, field.offset, field.size
                )),
            ]
            .spacing(10)
            .align_y(Alignment::Center),
            choices,
        ]
        .spacing(8),
        !unchanged,
    )
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
    let selected = editor.rollback_choice();
    // "0" duplicates "Current" when every stored index is already 0.
    let all_zero = lapiswitch_core::deviceinfo::indices(editor.layout, &editor.before)
        .iter()
        .all(|index| *index == 0);
    let mut choices = row![choice(
        if all_zero {
            format!("{} (0)", app.t("현재값", "Current"))
        } else {
            app.t("현재값", "Current").into()
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
        app.t("사용자지정", "Custom").into(),
        selected == RollbackChoice::Custom,
        EditorMessage::Rollback(RollbackChoice::Custom),
    ));
    let mut content = column![
        row![
            label(app.t("롤백인덱스 32개", "32 rollback indices")).size(15),
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

fn kind_hint(app: &App, field: &Field) -> String {
    match field.kind {
        Kind::Flag => app.t("0 또는 1", "0 or 1").into(),
        Kind::Uint => app
            .t(
                "{bytes}바이트 정수 (10진수 또는 0x16진수)",
                "{bytes}-byte integer (decimal or 0x hex)",
            )
            .replace("{bytes}", &field.size.to_string()),
        Kind::Region => "PRC / ROW".into(),
        Kind::Text { nul } => app
            .t("ASCII 최대 {chars}자", "ASCII, up to {chars} characters")
            .replace("{chars}", &(field.size - usize::from(nul)).to_string()),
        Kind::Bytes => app
            .t("16진수 {bytes}바이트", "{bytes} bytes of hex")
            .replace("{bytes}", &field.size.to_string()),
    }
}

/// Long values (keys, caches) are summarized in the review list.
fn short(value: &str) -> String {
    if value.chars().count() > 48 {
        let head: String = value.chars().take(40).collect();
        format!("{head}… ({}자)", value.chars().count())
    } else {
        value.to_owned()
    }
}

fn field_row<'a>(app: &'a App, editor: &'a Editor, field: &'a Field) -> Element<'a, Message> {
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
            button(small(app.t("원래 값", "Original")))
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
            app.t("원래 값", "Original"),
            short(&editor.original(field))
        )));
    }
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
        (_, true) => app.t("펌웨어 검사 중…", "Checking firmware…").into(),
        (Some(Ok(report)), _) => {
            format!("{} · {}", app.t("검사 완료", "Checked"), report.fingerprint)
        }
        (Some(Err(error)), _) => error.clone(),
        (None, _) => app
            .t("펌웨어 폴더를 선택하세요.", "Choose a firmware folder.")
            .into(),
    };
    Some(
        column![
            note(
                app.t(
                    "리전 변경 {from} → {to}: 해당 리전의 공식 펌웨어를 설치해야 정상 부팅됩니다. 펌웨어 설치 시 사용자 데이터가 초기화됩니다.",
                    "Region {from} → {to}: the official firmware of that region must be installed to boot. Installing firmware erases user data."
                )
                .replace("{from}", from)
                .replace("{to}", to),
                true
            ),
            row![
                action(
                    app.t("펌웨어 폴더 선택", "Choose firmware folder"),
                    Message::Pick(Picker::EditorFirmware),
                    !editor.checking
                )
                .style(soft_style),
                small(status).width(Fill),
            ]
            .spacing(12)
            .align_y(Alignment::Center),
            row![
                small(app.t("국가 코드", "Country code")),
                label(country_label(app, editor)).width(Fill),
                action(app.t("선택", "Choose"), Message::CountryOpen, true).style(soft_style),
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
        None => app.t("변경 안 함 (기본)", "Do not change (default)").into(),
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
            label(app.t("변경 내용 확인", "Review changes")).size(25),
            small(
                app.t("바뀌는 필드 {count}개", "{count} fields change")
                    .replace("{count}", &changes.len().to_string())
            ),
            scrollable(list)
                .direction(super::fluent::thin_scrollbar())
                .style(super::fluent::scroll_style)
                .height(if flash { 340 } else { 380 }),
            if flash {
                row![
                    small(app.t("국가 코드", "Country code")),
                    label(country_label(app, editor)),
                ]
                .spacing(12)
            } else {
                row![]
            },
            note(
                if flash {
                    app.t(
                        "적용하면 DeviceInfo를 쓰고 검증한 뒤 원본 GPT·Recovery를 복원하고 공식 펌웨어를 설치합니다. 사용자 데이터가 초기화됩니다.",
                        "Applying writes and verifies DeviceInfo, restores the original GPT/Recovery and installs official firmware. User data is erased.",
                    )
                } else {
                    app.t(
                        "적용하면 DeviceInfo를 쓰고 검증한 뒤 원본 GPT·Recovery를 복원하고 재부팅합니다. 펌웨어 설치와 데이터 초기화는 하지 않습니다.",
                        "Applying writes and verifies DeviceInfo, restores the original GPT/Recovery and reboots. No firmware is installed and no data is erased.",
                    )
                },
                flash
            ),
            row![
                space().width(Fill),
                action(app.t("이전", "Back"), Message::Editor(EditorMessage::Review(false)), true)
                    .style(soft_style),
                action(app.t("적용", "Apply"), Message::Editor(EditorMessage::Apply), true)
                    .style(if editor.protected_changed() || flash { super::fluent::danger_style } else { super::fluent::accent_style }),
            ]
            .spacing(12),
        ]
        .spacing(16),
    )
    .max_width(820)
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
            .push(choice_row(app, editor, "region", &["PRC", "ROW"]))
            .push(choice_row(app, editor, LOCK_KEYS[0], &["0", "1"]))
            .push(choice_row(app, editor, LOCK_KEYS[1], &["0", "1"]))
            .push(rollback_row(app, editor));
        if LOCK_KEYS.iter().any(|key| !editor.unchanged(key)) {
            list = list.push(note(
                app.t(
                    "잠금 상태 값은 부트로더 동작과 보안 상태에 영향을 줍니다. 기기에 맞는 값인지 확인하세요.",
                    "Lock state values affect bootloader behaviour and the security state. Make sure they suit the device.",
                ),
                true,
            ));
        }
    } else {
        list = list.push(note(
            app.t(
                "모든 필드를 직접 입력합니다. 잠금 상태, verity, 공개키, AVB 값, 기기 고유 캐시와 구조 표식을 잘못 바꾸면 부팅되지 않거나 보안 상태가 바뀔 수 있습니다.",
                "Every field is typed directly. Wrong lock state, verity, public key, AVB values, device-unique caches or structure markers can prevent booting or change the security state.",
            ),
            true,
        ));
        // The key, the English description and the shown translation all match.
        let query = editor.query.trim().to_lowercase();
        let language = app.preferences.language;
        for field in editor.fields.iter().filter(|field| {
            query.is_empty()
                || field.key.to_lowercase().contains(&query)
                || field.description.to_lowercase().contains(&query)
                || language
                    .field_description(field.description)
                    .to_lowercase()
                    .contains(&query)
        }) {
            list = list.push(field_row(app, editor, field));
        }
    }
    let mut body = column![
        label(
            app.t("DeviceInfo 수정 · {model}", "Edit DeviceInfo · {model}")
                .replace("{model}", editor.layout.model())
        )
        .size(25),
        small(
            app.t(
                "TWRP에서 읽은 현재 값입니다. 바꿀 필드만 수정하세요. 논리 크기 0x{size} 밖의 공유 버퍼는 바뀌지 않습니다.",
                "Current values read in TWRP. Change only the fields you need. The shared buffer after the 0x{size} logical size is never changed.",
            )
            .replace("{size}", &format!("{:X}", editor.layout.logical_size()))
        ),
        if editor.tab == Tab::Custom {
            row![
                tabs,
                space().width(Fill),
                text_input(app.t("필드 검색", "Search fields"), &editor.query)
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
    if let Some(region) = region_box(app, editor) {
        body = body.push(region);
    }
    if editor.protected_changed() {
        body = body.push(
            checkbox(editor.protected_ack)
                .label(app.t(
                    "보안·구조 필드를 바꾸면 부팅 불가·보안 상태 변경이 생길 수 있음을 이해했습니다.",
                    "I understand that changing security/structure fields can prevent booting or change the security state.",
                ))
                .on_toggle(|ack| Message::Editor(EditorMessage::Ack(ack))),
        );
    }
    let blocker = editor.blocker(app);
    body = body.push(
        row![
            small(blocker.unwrap_or("")).width(Fill),
            action(
                app.t("모두 원래 값", "Reset all"),
                Message::Editor(EditorMessage::ResetAll),
                true
            )
            .style(soft_style),
            action(
                app.t("작업 취소 · 원본 복구", "Cancel · Restore originals"),
                Message::Editor(EditorMessage::Cancel),
                true
            )
            .style(soft_style),
            action(
                app.t("변경 내용 확인", "Review changes"),
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
