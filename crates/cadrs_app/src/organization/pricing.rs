//! The organization screen's Stock and Prices modules: sheet on hand and what is held for
//! orders; the rate card instant quotes are made from.

use super::*;

pub(super) fn stock(b: &mut ChildSpawnerCommands, t: &Theme, v: &ModuleView) {
    let org = &v.org;
    page_head(b, t, "org-tab-stock", "org-stock-about");
    let columns = vec![
        Column::new("material", tr("org-material-name")).width(220.0),
        Column::new("sheet", tr("org-material-sheet")).width(150.0),
        Column::new("on-hand", tr("org-stock-on-hand")).width(90.0),
        Column::new("free", tr("org-stock-free")).width(90.0),
        Column::new("actions", "").width(210.0),
    ];
    let rows = org
        .materials
        .iter()
        .enumerate()
        .map(|(i, (id, m))| {
            let id = *id;
            let on_hand = org.stock.get(&id).copied().unwrap_or(0);
            let free = org.available(id);
            let held = org.reservations.values().any(|r| r.material == id);
            TableRow::new(format!("org-material-{i}"), &columns)
                .text_cell(t, format!("{} {} mm", m.name, m.thickness))
                .text_cell(t, format!("{} × {} mm", m.sheet[0], m.sheet[1]))
                .cell({
                    let t = t.clone();
                    move |c| {
                        c.spawn((
                            Name::new(format!("org-material-count-{i}")),
                            t.text(
                                on_hand.to_string(),
                                t.font_base,
                                FontWeight::MEDIUM,
                                t.foreground,
                            ),
                        ));
                    }
                })
                .text_cell(t, free.to_string())
                .cell(move |c| {
                    for (delta, label) in [(1i64, "+1"), (-1, "−1")] {
                        cell_button(
                            c,
                            format!("org-material-{i}-{delta}"),
                            label.to_owned(),
                            false,
                            move |w| {
                                write(
                                    w,
                                    Op::Stock {
                                        material: id,
                                        sheets: delta,
                                    },
                                );
                            },
                        );
                    }
                    // Not while sheets of it are held for an order.
                    if !held {
                        cell_button(
                            c,
                            format!("org-material-remove-{i}"),
                            tr("common-remove"),
                            false,
                            move |w| ask_remove(w, Op::SetMaterial { id, material: None }),
                        );
                    }
                })
        })
        .collect();
    table(b, t, "org-materials", columns, rows, "org-no-materials");
    b.spawn((
        Button::new("org-add-material")
            .label(tr("org-add-material"))
            .icon("plus")
            .build(t),
        observe(
            |_: On<Activate>,
             mut commands: Commands,
             theme: Res<Theme>,
             open: Option<Res<OpenOrg>>| {
                let t = theme.clone();
                let tb = theme.clone();
                let names = open.map_or_else(Vec::new, |o| material_names(o.session.org()));
                commands.spawn((
                    Dialog::new("org-material-dialog")
                        .title(tr("org-add-material"))
                        .width(520.0)
                        .body(move |b| {
                            let t = &tb;
                            b.spawn(t_text(t, "org-material-name"));
                            // Picked from the Part Studio's materials, so that a part's
                            // material finds the sheet it is cut from.
                            b.spawn(material_picker("org-material-name", names).build(t));
                            b.spawn(t_text(t, "org-material-size"));
                            b.spawn(Node {
                                column_gap: Val::Px(8.0),
                                ..default()
                            })
                            .with_children(|r| {
                                for (name, value) in [
                                    ("org-material-thickness", "3"),
                                    ("org-material-x", "2500"),
                                    ("org-material-y", "1250"),
                                ] {
                                    r.spawn(
                                        TextInput::new(name)
                                            .value(value)
                                            .width(Val::Px(100.0))
                                            .build(t),
                                    );
                                }
                            });
                        })
                        .footer(|f| dialog_buttons(f, "org-material", add_material))
                        .build(&t),
                    OrgDialog,
                    DespawnOnExit(AppState::Organization),
                ));
            },
        ),
    ));
}

pub(super) fn add_material(world: &mut World) {
    let name = picked(world, "org-material-name").unwrap_or_default();
    let Some(thickness) = number(world, "org-material-thickness").filter(|t| *t > 0.0) else {
        crate::linked::toast(world, tr("org-material-needs-thickness"));
        return;
    };
    if name.trim().is_empty() {
        crate::linked::toast(world, tr("org-material-needs-name"));
        return;
    }
    let sheet = [
        number(world, "org-material-x").unwrap_or(0.0),
        number(world, "org-material-y").unwrap_or(0.0),
    ];
    if write(
        world,
        Op::SetMaterial {
            id: new_id(),
            material: Some(Material {
                name: name.trim().to_owned(),
                thickness,
                sheet,
            }),
        },
    ) {
        close_dialogs(world);
    }
}

pub(super) fn money_text(m: Money) -> String {
    format!("{:.2}", m.0 as f64 / 100.0)
}

/// Whether `code` is shaped like a currency's (ISO 4217): three letters.
pub(crate) fn currency_code(text: &str) -> Option<String> {
    let code = text.trim().to_uppercase();
    (code.len() == 3 && code.chars().all(|c| c.is_ascii_alphabetic())).then_some(code)
}

pub(super) fn prices(b: &mut ChildSpawnerCommands, t: &Theme, v: &ModuleView) {
    let org = &v.org;
    let rates = org.rates.clone().unwrap_or_else(|| RateCard {
        currency: String::new(),
        setup: Money::default(),
        minimum: Money::default(),
        hourly: Vec::new(),
        sheet_per_m2: Vec::new(),
        nesting_allowance: 1.25,
    });
    page_head(b, t, "org-tab-prices", "org-prices-help");
    muted(b, t, tr("org-rates-currency-credits"));
    row(b, |r| {
        r.spawn(t_text(t, "org-rates-currency"));
        // Credits unless the organization says otherwise: what its clients pay with here.
        let currency = if rates.currency.trim().is_empty() {
            cadrs_org::quote::CREDITS.to_owned()
        } else {
            rates.currency.clone()
        };
        input(r, t, "org-rates-currency", currency, 80.0);
        r.spawn(t_text(t, "org-rates-setup"));
        input(r, t, "org-rates-setup", money_text(rates.setup), 110.0);
        r.spawn(t_text(t, "org-rates-minimum"));
        input(r, t, "org-rates-minimum", money_text(rates.minimum), 110.0);
        r.spawn(t_text(t, "org-rates-nesting"));
        input(
            r,
            t,
            "org-rates-nesting",
            format!("{:.0}", (rates.nesting_allowance - 1.0) * 100.0),
            70.0,
        );
    });
    subheading(b, t, "org-rates-hourly");
    if org.machines.is_empty() {
        muted(b, t, tr("org-no-machines"));
    }
    for (i, (id, m)) in org.machines.iter().enumerate() {
        let rate = rates.hourly(*id).map_or(String::new(), money_text);
        row(b, |r| {
            r.spawn(
                TextInput::new(format!("org-rate-machine-{i}"))
                    .value(rate)
                    .width(Val::Px(110.0))
                    .build(t),
            );
            r.spawn(t.text(
                m.name.clone(),
                t.font_base,
                FontWeight::NORMAL,
                t.foreground,
            ));
        });
    }
    subheading(b, t, "org-rates-sheet");
    if org.materials.is_empty() {
        muted(b, t, tr("org-no-materials"));
    }
    for (i, m) in org.materials.values().enumerate() {
        let price = rates
            .sheet_per_m2
            .iter()
            .find(|s| {
                s.material.eq_ignore_ascii_case(&m.name) && (s.thickness - m.thickness).abs() < 1e-6
            })
            .map_or(String::new(), |s| money_text(s.per_m2));
        row(b, |r| {
            r.spawn(
                TextInput::new(format!("org-rate-material-{i}"))
                    .value(price)
                    .width(Val::Px(110.0))
                    .build(t),
            );
            r.spawn(t.text(
                format!("{} {} mm", m.name, m.thickness),
                t.font_base,
                FontWeight::NORMAL,
                t.foreground,
            ));
        });
    }
    b.spawn((
        Button::new("org-rates-save")
            .label(tr("common-save"))
            .primary()
            .build(t),
        observe(|_: On<Activate>, mut commands: Commands| {
            commands.queue(save_prices);
        }),
    ));
}

pub(super) fn save_prices(world: &mut World) {
    let money = |world: &mut World, name: &str| {
        number(world, name)
            .filter(|v| *v >= 0.0)
            .map(Money::from_major)
    };
    let Some(currency) = currency_code(&field(world, "org-rates-currency")) else {
        crate::linked::toast(world, tr("org-rates-needs-currency"));
        return;
    };
    let setup = money(world, "org-rates-setup").unwrap_or_default();
    let minimum = money(world, "org-rates-minimum").unwrap_or_default();
    let nesting = 1.0 + number(world, "org-rates-nesting").unwrap_or(25.0).max(0.0) / 100.0;
    let Some(open) = world.get_resource::<OpenOrg>() else {
        return;
    };
    let machines: Vec<cadrs_org::Id> = open.session.org().machines.keys().copied().collect();
    let materials: Vec<(String, f64)> = open
        .session
        .org()
        .materials
        .values()
        .map(|m| (m.name.clone(), m.thickness))
        .collect();
    let mut hourly = Vec::new();
    for (i, id) in machines.into_iter().enumerate() {
        if let Some(rate) = money(world, &format!("org-rate-machine-{i}")) {
            hourly.push(cadrs_org::quote::MachineRate::new(id, rate));
        }
    }
    let mut sheet_per_m2 = Vec::new();
    for (i, (name, thickness)) in materials.into_iter().enumerate() {
        if let Some(price) = money(world, &format!("org-rate-material-{i}")) {
            sheet_per_m2.push(cadrs_org::quote::SheetPrice::new(name, thickness, price));
        }
    }
    let rates = RateCard {
        currency,
        setup,
        minimum,
        hourly,
        sheet_per_m2,
        nesting_allowance: nesting,
    };
    if write(world, Op::SetRates(rates)) {
        crate::linked::toast(world, tr("org-saved"));
    }
}
