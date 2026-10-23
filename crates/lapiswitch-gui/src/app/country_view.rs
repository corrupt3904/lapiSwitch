use super::view::*;
use super::*;
use iced::widget::{self, button, column, row, scrollable, space, svg, text_input};
use iced::{Alignment, Fill};

fn choice<'a>(app: &'a App, code: &str, name: &str, korean: &str) -> Element<'a, Message> {
    let selected = app.country_draft.as_deref() == Some(code);
    let mut content = row![
        label(if selected { "●" } else { "○" })
            .size(17)
            .color(accent())
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
            style.background = Some(if selected { pale() } else { card_color() }.into());
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
            app.tr("country_do_not_change"),
            app.tr("country_keep_the_current_country_code"),
        ));
    }
    let found: Vec<_> = crate::countries::search(&app.country_search).collect();
    if found.is_empty() {
        choices = choices.push(note(app.tr("country_no_matching_countries"), false));
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
            label(app.tr("country_choose_country_code")).size(25),
            text_input(
                app.tr("country_search_country_or_code_e_g"),
                &app.country_search
            )
            .on_input(Message::CountrySearch)
            .padding(12),
            scrollable(choices)
                .direction(super::fluent::thin_scrollbar())
                .style(super::fluent::scroll_style)
                .height(300),
            small(format!(
                "{}: {}",
                app.tr("country_selected_code"),
                app.country_draft.as_deref().unwrap_or("—")
            )),
            row![
                space().width(Fill),
                action(app.tr("archive_cancel"), Message::CountryCancel, true).style(soft_style),
                action(
                    app.tr("country_select"),
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
