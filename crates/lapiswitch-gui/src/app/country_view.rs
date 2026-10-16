use super::view::*;
use super::*;
use iced::widget::{self, button, column, row, scrollable, space, svg, text_input};
use iced::{Alignment, Color, Fill};

fn choice<'a>(app: &'a App, code: &str, name: &str, korean: &str) -> Element<'a, Message> {
    let selected = app.country_draft.as_deref() == Some(code);
    let mut content = row![
        label(if selected { "●" } else { "○" })
            .size(17)
            .color(ACCENT)
    ]
    .spacing(13)
    .align_y(Alignment::Center);
    if code != "—" {
        content = content.push(svg(crate::countries::flag(code)).width(29).height(22));
    }
    content = content
        .push(label(code).size(15).width(38))
        .push(column![label(name).size(15), small(korean)].spacing(2));
    button(content)
        .on_press(Message::Country(code.into()))
        .padding([10, 14])
        .width(Fill)
        .style(move |theme, state| {
            let mut style = button::secondary(theme, state);
            style.background = Some(if selected { PALE } else { Color::WHITE }.into());
            style
        })
        .into()
}
pub(super) fn modal(app: &App) -> widget::Container<'_, Message> {
    let mut choices = column![].spacing(4);
    if app.country_search.trim().is_empty() {
        choices = choices.push(choice(
            app,
            "—",
            app.t("변경 안 함", "Do not change"),
            app.t("기존 국가 코드 유지", "Keep the current country code"),
        ));
    }
    let found: Vec<_> = crate::countries::search(&app.country_search).collect();
    if found.is_empty() {
        choices = choices.push(note(
            app.t("검색 결과가 없습니다.", "No matching countries."),
            false,
        ));
    }
    for country in found {
        choices = choices.push(choice(
            app,
            country.code,
            country.name,
            country.local_name(app.preferences.language),
        ));
    }
    panel(
        column![
            label(app.t("국가 코드 선택", "Choose country code")).size(25),
            text_input(
                app.t(
                    "국가명 또는 코드 검색 (예: 한국, Korea, KR)",
                    "Search country or code (e.g. Korea, KR)"
                ),
                &app.country_search
            )
            .on_input(Message::CountrySearch)
            .padding(12),
            scrollable(choices).height(300),
            small(format!(
                "{}: {}",
                app.t("선택한 코드", "Selected code"),
                app.country_draft.as_deref().unwrap_or("—")
            )),
            row![
                space().width(Fill),
                action(app.t("취소", "Cancel"), Message::CountryCancel, true).style(soft_style),
                action(
                    app.t("적용", "Apply"),
                    Message::CountryApply,
                    app.country_draft.is_some()
                )
            ]
            .spacing(12),
        ]
        .spacing(16),
    )
    .max_width(590)
}
pub(super) fn selected(app: &App) -> Element<'_, Message> {
    let mut content = row![].spacing(12).align_y(Alignment::Center);
    if let Some(country) = app.country.as_deref().and_then(crate::countries::find) {
        content = content
            .push(
                svg(crate::countries::flag(country.code))
                    .width(30)
                    .height(23),
            )
            .push(label(format!("{} · {}", country.code, country.name)));
    } else {
        content = content.push(label(if app.country.is_some() {
            app.t("국가 코드 변경 안 함", "Country code unchanged")
        } else {
            app.t("국가 코드를 선택하세요", "Choose a country code")
        }));
    }
    content
        .push(
            action(app.t("선택", "Choose"), Message::CountryOpen, !app.busy()).style(
                if app.country.is_none() {
                    button::primary
                } else {
                    soft_style
                },
            ),
        )
        .into()
}
