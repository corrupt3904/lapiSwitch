use super::view::*;
use super::*;
use iced::widget::{button, column, container, row, scrollable, space, text, text_input};
use iced::{Alignment, Border, Fill};

pub(super) fn modal(app: &App) -> iced::widget::Container<'_, Message> {
    let catalog = crate::licenses::catalog();
    let mut choices = column![].spacing(4);
    let mut count = 0;
    for (index, component) in catalog.components.iter().enumerate() {
        if !component.matches(&app.license_search) {
            continue;
        }
        count += 1;
        let selected = index == app.license_selection;
        choices = choices.push(
            button(
                column![
                    label(&component.name).size(14),
                    small(format!("{} · {}", component.version, component.license)).size(11),
                ]
                .spacing(4),
            )
            .on_press(Message::LicenseSelect(index))
            .padding(10)
            .width(Fill)
            .style(move |theme, status| {
                let mut style = soft_style(theme, status);
                if selected {
                    style.background = Some(pale().into());
                    style.border.color = accent();
                }
                style
            }),
        );
    }
    if count == 0 {
        choices = choices.push(small(app.tr("licenses_no_matching_components")));
    }
    let selected = &catalog.components[app.license_selection];
    let mut documents = column![
        label(&selected.name).size(22),
        small(format!("{} · {}", selected.version, selected.license)),
        small(&selected.source)
            .width(Fill)
            .wrapping(iced::widget::text::Wrapping::WordOrGlyph),
    ]
    .spacing(12);
    for notice in &selected.notices {
        documents = documents
            .push(label(&notice.title).size(15))
            // Original notices remain verbatim; don't translate or normalize paths.
            .push(
                text(catalog.texts[notice.text].as_str())
                    .shaping(iced::widget::text::Shaping::Advanced)
                    .size(13)
                    .width(Fill)
                    .wrapping(iced::widget::text::Wrapping::WordOrGlyph)
                    .color(ink()),
            );
    }
    let reader = container(
        scrollable(container(documents).width(Fill).padding([0, 12]))
            .direction(super::fluent::thin_scrollbar())
            .style(super::fluent::scroll_style)
            .id(format!("license-document-{}", app.license_selection))
            .width(Fill)
            .height(Fill),
    )
    .padding(16)
    .width(Fill)
    .height(Fill)
    .style(|_| container::Style {
        background: Some(card_color().into()),
        border: Border {
            color: line(),
            width: 1.0,
            radius: 8.0.into(),
        },
        ..Default::default()
    });
    panel(
        column![
            label(app.tr("licenses_open_source_licenses")).size(25),
            small(app.tr("licenses_select_a_component_to_read_its")),
            row![
                column![
                    text_input(
                        app.tr("licenses_search_components_or_licenses"),
                        &app.license_search
                    )
                    .on_input(Message::LicenseSearch)
                    .padding(10),
                    scrollable(choices)
                        .direction(super::fluent::thin_scrollbar())
                        .style(super::fluent::scroll_style)
                        .height(Fill),
                ]
                .spacing(12)
                .width(230),
                reader,
            ]
            .spacing(16)
            .height(Fill),
            row![
                space().width(Fill),
                action(app.tr("licenses_close"), Message::LicenseClose, true).style(soft_style)
            ]
            .align_y(Alignment::Center),
        ]
        .spacing(16),
    )
    .max_width(980)
    .height(iced::Length::FillPortion(9))
    .max_height(700)
}
