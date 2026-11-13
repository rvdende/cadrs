//! The organization screen's running-the-shop tabs: Jobs (a schedule per machine), Clients and
//! Parts (with what goes into them). Each change is an operation on the organization document
//! ([`crate::organization::write`]).

use bevy::prelude::*;
use bevy::text::FontWeight;
use bevy::ui_widgets::{Activate, observe};
use cadrs_org::{BomLine, Client, Job, JobState, Op, Part, Stage, new_id};
use cadrs_ui::Button;
use cadrs_ui::dialog::Dialog;
use cadrs_ui::i18n::tr;
use cadrs_ui::input::TextInput;
use cadrs_ui::tab_strip::TabStrip;
use cadrs_ui::theme::Theme;

use crate::AppState;
use crate::organization::{
    OpenOrg, OrgDialog, close_dialogs, dialog_buttons, field, heading, muted, number, row, short,
    strip_selected, t_text, write,
};

/// "2026-11-14 08:00" in local time ↔ milliseconds since the Unix epoch.
fn parse_when(text: &str) -> Option<u64> {
    let naive = chrono::NaiveDateTime::parse_from_str(text.trim(), "%Y-%m-%d %H:%M").ok()?;
    let local = naive.and_local_timezone(chrono::Local).single()?;
    u64::try_from(local.timestamp_millis()).ok()
}

fn show_when(millis: u64) -> String {
    chrono::DateTime::from_timestamp_millis(millis as i64)
        .map(|t| {
            t.with_timezone(&chrono::Local)
                .format("%Y-%m-%d %H:%M")
                .to_string()
        })
        .unwrap_or_default()
}

/// The orders that can be scheduled: accepted and not yet delivered.
fn open_orders(open: &OpenOrg) -> Vec<(cadrs_org::Id, String, f64)> {
    open.session
        .org()
        .requests
        .iter()
        .filter(|(_, r)| {
            matches!(
                r.stage,
                Stage::Accepted | Stage::InProduction | Stage::Ready
            )
        })
        .map(|(id, r)| {
            let parts: Vec<String> = r
                .parts
                .iter()
                .map(|(p, n)| format!("{n} × {}", p.name))
                .collect();
            let minutes = r
                .quote
                .as_ref()
                .map(|q| q.lines.iter().map(|l| l.minutes).sum())
                .unwrap_or(60.0);
            (*id, parts.join(", "), minutes)
        })
        .collect()
}

pub(crate) fn jobs(b: &mut ChildSpawnerCommands, t: &Theme, open: &OpenOrg) {
    let org = open.session.org();
    heading(b, t, "org-tab-jobs");
    if org.machines.is_empty() {
        muted(b, t, tr("org-no-machines"));
        return;
    }
    let mut i = 0;
    for (machine_id, machine) in &org.machines {
        b.spawn(t.text(
            machine.name.clone(),
            t.font_md,
            FontWeight::BOLD,
            t.foreground,
        ));
        let mut jobs: Vec<(&cadrs_org::Id, &Job)> = org
            .jobs
            .iter()
            .filter(|(_, j)| j.machine == *machine_id)
            .collect();
        jobs.sort_by_key(|(_, j)| j.start);
        if jobs.is_empty() {
            muted(b, t, tr("org-no-jobs"));
        }
        for (id, job) in jobs {
            let (id, job) = (*id, job.clone());
            let what = org
                .requests
                .get(&job.request)
                .map(|r| {
                    r.parts
                        .iter()
                        .map(|(p, n)| format!("{n} × {}", p.name))
                        .collect::<Vec<_>>()
                        .join(", ")
                })
                .unwrap_or_default();
            let state = tr(match job.state {
                JobState::Scheduled => "org-job-scheduled",
                JobState::Running => "org-job-running",
                JobState::Done => "org-job-done",
            });
            let next = match job.state {
                JobState::Scheduled => Some((JobState::Running, "org-job-start")),
                JobState::Running => Some((JobState::Done, "org-job-finish")),
                JobState::Done => None,
            };
            let n = i;
            i += 1;
            row(b, |r| {
                r.spawn((
                    Name::new(format!("org-job-{n}")),
                    t.text(
                        format!("{} · {:.0} min", show_when(job.start), job.minutes),
                        t.font_base,
                        FontWeight::MEDIUM,
                        t.foreground,
                    ),
                ));
                r.spawn(t.text(what, t.font_sm, FontWeight::NORMAL, t.foreground));
                r.spawn((
                    Name::new(format!("org-job-state-{n}")),
                    t.text(state, t.font_sm, FontWeight::BOLD, t.foreground),
                ));
                if let Some((to, label)) = next {
                    r.spawn((
                        Button::new(format!("org-job-next-{n}"))
                            .label(tr(label))
                            .small()
                            .build(t),
                        observe(move |_: On<Activate>, mut commands: Commands| {
                            let mut job = job.clone();
                            commands.queue(move |w: &mut World| {
                                job.state = to;
                                write(w, Op::SetJob { id, job: Some(job) });
                            });
                        }),
                    ));
                }
            });
        }
    }
    let orders = open_orders(open);
    if orders.is_empty() {
        muted(b, t, tr("org-no-orders-to-schedule"));
        return;
    }
    let machines: Vec<(cadrs_org::Id, String)> = org
        .machines
        .iter()
        .map(|(id, m)| (*id, m.name.clone()))
        .collect();
    b.spawn((
        Button::new("org-add-job")
            .label(tr("org-add-job"))
            .icon("plus")
            .build(t),
        observe(
            move |_: On<Activate>, mut commands: Commands, theme: Res<Theme>| {
                let t = theme.clone();
                let tb = theme.clone();
                let orders = orders.clone();
                let machines = machines.clone();
                commands.spawn((
                    Dialog::new("org-job-dialog")
                        .title(tr("org-add-job"))
                        .width(600.0)
                        .body(move |b| {
                            let t = &tb;
                            b.spawn(t_text(t, "org-job-order"));
                            let mut strip = TabStrip::new("org-job-order");
                            for (_, what, _) in &orders {
                                strip = strip.tab(what.clone());
                            }
                            b.spawn(strip.build(t));
                            b.spawn(t_text(t, "org-job-machine"));
                            let mut strip = TabStrip::new("org-job-machine");
                            for (_, name) in &machines {
                                strip = strip.tab(name.clone());
                            }
                            b.spawn(strip.build(t));
                            b.spawn(t_text(t, "org-job-when"));
                            let start = show_when(cadrs_social::wall_millis() + 3_600_000);
                            let minutes = format!("{:.0}", orders[0].2.max(1.0));
                            b.spawn(Node {
                                column_gap: Val::Px(8.0),
                                ..default()
                            })
                            .with_children(|r| {
                                r.spawn(
                                    TextInput::new("org-job-start")
                                        .value(start)
                                        .width(Val::Px(180.0))
                                        .build(t),
                                );
                                r.spawn(
                                    TextInput::new("org-job-minutes")
                                        .value(minutes)
                                        .width(Val::Px(100.0))
                                        .build(t),
                                );
                            });
                        })
                        .footer(|f| dialog_buttons(f, "org-job", add_job))
                        .build(&t),
                    OrgDialog,
                    DespawnOnExit(AppState::Organization),
                ));
            },
        ),
    ));
}

fn add_job(world: &mut World) {
    let Some(open) = world.get_resource::<OpenOrg>() else {
        return;
    };
    let orders = open_orders(open);
    let machines: Vec<cadrs_org::Id> = open.session.org().machines.keys().copied().collect();
    let order = orders
        .get(strip_selected(world, "org-job-order"))
        .map(|o| o.0);
    let machine = machines
        .get(strip_selected(world, "org-job-machine"))
        .copied();
    let start = parse_when(&field(world, "org-job-start"));
    let minutes = number(world, "org-job-minutes").unwrap_or(60.0);
    let (Some(request), Some(machine), Some(start)) = (order, machine, start) else {
        crate::linked::toast(world, tr("org-job-bad"));
        return;
    };
    let job = Job {
        request,
        machine,
        start,
        minutes,
        state: JobState::Scheduled,
    };
    if write(
        world,
        Op::SetJob {
            id: new_id(),
            job: Some(job),
        },
    ) {
        close_dialogs(world);
    }
}

pub(crate) fn clients(
    b: &mut ChildSpawnerCommands,
    t: &Theme,
    open: &OpenOrg,
    social: &crate::social::Social,
) {
    let org = open.session.org();
    heading(b, t, "org-tab-clients");
    // Everyone who sent a request, with their orders.
    let mut by_account: std::collections::BTreeMap<_, (usize, usize)> = Default::default();
    for r in org.requests.values() {
        let e = by_account.entry(r.client).or_default();
        e.0 += 1;
        if r.stage == Stage::Completed {
            e.1 += 1;
        }
    }
    let mut i = 0;
    for (account, (requests, completed)) in &by_account {
        let name = org
            .clients
            .values()
            .find(|c| c.account == Some(*account))
            .map(|c| c.name.clone())
            .or_else(|| social.contacts.get(account).map(|c| c.name.clone()))
            .unwrap_or_else(|| short(account));
        let n = i;
        i += 1;
        row(b, |r| {
            r.spawn((
                Name::new(format!("org-client-{n}")),
                t.text(name, t.font_base, FontWeight::MEDIUM, t.foreground),
            ));
            r.spawn(t.text(
                format!(
                    "{requests} {} · {completed} {}",
                    tr("org-client-requests"),
                    tr("org-client-completed")
                ),
                t.font_sm,
                FontWeight::NORMAL,
                t.muted_foreground,
            ));
        });
    }
    // Clients kept by hand (walk-ins, phone orders).
    for client in org.clients.values().filter(|c| c.account.is_none()) {
        let n = i;
        i += 1;
        row(b, |r| {
            r.spawn((
                Name::new(format!("org-client-{n}")),
                t.text(
                    client.name.clone(),
                    t.font_base,
                    FontWeight::MEDIUM,
                    t.foreground,
                ),
            ));
            r.spawn(
                t.text(
                    [
                        client.email.as_str(),
                        client.phone.as_str(),
                        client.notes.as_str(),
                    ]
                    .into_iter()
                    .filter(|s| !s.is_empty())
                    .collect::<Vec<_>>()
                    .join(" · "),
                    t.font_sm,
                    FontWeight::NORMAL,
                    t.muted_foreground,
                ),
            );
        });
    }
    if i == 0 {
        muted(b, t, tr("org-no-clients"));
    }
    b.spawn((
        Button::new("org-add-client")
            .label(tr("org-add-client"))
            .icon("plus")
            .build(t),
        observe(
            |_: On<Activate>, mut commands: Commands, theme: Res<Theme>| {
                let t = theme.clone();
                let tb = theme.clone();
                commands.spawn((
                    Dialog::new("org-client-dialog")
                        .title(tr("org-add-client"))
                        .width(520.0)
                        .body(move |b| {
                            let t = &tb;
                            for name in [
                                "org-client-name",
                                "org-client-email",
                                "org-client-phone",
                                "org-client-notes",
                            ] {
                                b.spawn(t_text(t, name));
                                b.spawn(TextInput::new(name).build(t));
                            }
                        })
                        .footer(|f| dialog_buttons(f, "org-client", add_client))
                        .build(&t),
                    OrgDialog,
                    DespawnOnExit(AppState::Organization),
                ));
            },
        ),
    ));
}

fn add_client(world: &mut World) {
    let name = field(world, "org-client-name");
    if name.trim().is_empty() {
        crate::linked::toast(world, tr("org-client-needs-name"));
        return;
    }
    let client = Client {
        name: name.trim().to_owned(),
        account: None,
        email: field(world, "org-client-email").trim().to_owned(),
        phone: field(world, "org-client-phone").trim().to_owned(),
        notes: field(world, "org-client-notes").trim().to_owned(),
    };
    if write(
        world,
        Op::SetClient {
            id: new_id(),
            client: Some(client),
        },
    ) {
        close_dialogs(world);
    }
}

pub(crate) fn parts(b: &mut ChildSpawnerCommands, t: &Theme, open: &OpenOrg) {
    let org = open.session.org();
    heading(b, t, "org-tab-parts");
    if org.parts.is_empty() {
        muted(b, t, tr("org-no-parts"));
    }
    for (i, part) in org.parts.values().enumerate() {
        let bom: Vec<String> = part
            .bom
            .iter()
            .map(|l| match l {
                BomLine::Material { material, sheets } => org
                    .materials
                    .get(material)
                    .map(|m| format!("{sheets} × {} {} mm", m.name, m.thickness))
                    .unwrap_or_default(),
                BomLine::Part { part, quantity } => org
                    .parts
                    .get(part)
                    .map(|p| format!("{quantity} × {}", p.name))
                    .unwrap_or_default(),
                BomLine::Bought { name, quantity } => format!("{quantity} × {name}"),
            })
            .collect();
        row(b, |r| {
            r.spawn((
                Name::new(format!("org-part-{i}")),
                t.text(
                    format!("{} {}", part.name, part.revision),
                    t.font_base,
                    FontWeight::MEDIUM,
                    t.foreground,
                ),
            ));
            r.spawn(t.text(
                bom.join(", "),
                t.font_sm,
                FontWeight::NORMAL,
                t.muted_foreground,
            ));
        });
    }
    b.spawn((
        Button::new("org-add-part")
            .label(tr("org-add-part"))
            .icon("plus")
            .build(t),
        observe(
            |_: On<Activate>, mut commands: Commands, theme: Res<Theme>| {
                let t = theme.clone();
                let tb = theme.clone();
                commands.spawn((
                    Dialog::new("org-part-dialog")
                        .title(tr("org-add-part"))
                        .width(560.0)
                        .body(move |b| {
                            let t = &tb;
                            b.spawn(t_text(t, "org-part-name"));
                            b.spawn(TextInput::new("org-part-name").autofocus().build(t));
                            b.spawn(t_text(t, "org-part-revision"));
                            b.spawn(TextInput::new("org-part-revision").value("A").build(t));
                            b.spawn(t_text(t, "org-part-bom"));
                            b.spawn(
                                TextInput::new("org-part-bom")
                                    .placeholder(tr("org-part-bom-placeholder"))
                                    .build(t),
                            );
                        })
                        .footer(|f| dialog_buttons(f, "org-part", add_part))
                        .build(&t),
                    OrgDialog,
                    DespawnOnExit(AppState::Organization),
                ));
            },
        ),
    ));
}

/// "0.5 mild steel 10, 4 M8 bolt" → lines: a stock material (by name and thickness) by the
/// sheet, or something bought.
fn parse_bom(text: &str, org: &cadrs_org::Org) -> Vec<BomLine> {
    text.split([',', ';'])
        .filter_map(|item| {
            let item = item.trim();
            let (amount, rest) = item.split_once(' ')?;
            let amount: f64 = amount.replace(',', ".").parse().ok()?;
            let rest = rest.trim();
            // A stock material: its name then its thickness.
            let material = org.materials.iter().find(|(_, m)| {
                rest.eq_ignore_ascii_case(&format!("{} {}", m.name, m.thickness))
                    || rest.eq_ignore_ascii_case(&format!("{} {}mm", m.name, m.thickness))
                    || rest.eq_ignore_ascii_case(&format!("{} {} mm", m.name, m.thickness))
            });
            Some(match material {
                Some((id, _)) => BomLine::Material {
                    material: *id,
                    sheets: amount,
                },
                None => BomLine::Bought {
                    name: rest.to_owned(),
                    quantity: amount.round().max(1.0) as u32,
                },
            })
        })
        .collect()
}

fn add_part(world: &mut World) {
    let name = field(world, "org-part-name");
    if name.trim().is_empty() {
        crate::linked::toast(world, tr("org-part-needs-name"));
        return;
    }
    let revision = field(world, "org-part-revision");
    let bom_text = field(world, "org-part-bom");
    let Some(open) = world.get_resource::<OpenOrg>() else {
        return;
    };
    let bom = parse_bom(&bom_text, open.session.org());
    let part = Part {
        name: name.trim().to_owned(),
        revision: revision.trim().to_owned(),
        bom,
    };
    if write(
        world,
        Op::SetPart {
            id: new_id(),
            part: Some(part),
        },
    ) {
        close_dialogs(world);
    }
}

/// The channels to show: the ones with messages, and "general" always.
fn channel_names(open: &OpenOrg) -> Vec<String> {
    let mut names: Vec<String> = open.session.org().channels.keys().cloned().collect();
    if !names.iter().any(|n| n == "general") {
        names.insert(0, "general".into());
    }
    if !names.contains(&open.channel) {
        names.push(open.channel.clone());
    }
    names
}

pub(crate) fn chat(
    b: &mut ChildSpawnerCommands,
    t: &Theme,
    open: &OpenOrg,
    social: &crate::social::Social,
) {
    let org = open.session.org();
    heading(b, t, "org-tab-chat");
    let names = channel_names(open);
    let selected = names.iter().position(|n| *n == open.channel).unwrap_or(0);
    let mut strip = TabStrip::new("org-channels").compact().selected(selected);
    for n in &names {
        strip = strip.tab(format!("#{n}"));
    }
    b.spawn((
        strip.build(t),
        observe(
            |ev: On<cadrs_ui::tab_strip::TabStripSelect>, mut open: ResMut<OpenOrg>| {
                if let Some(name) = channel_names(&open).get(ev.index).cloned() {
                    open.channel = name;
                }
            },
        ),
    ));
    let me = social.keys().map(|k| k.account_id());
    let messages = org.channels.get(&open.channel).cloned().unwrap_or_default();
    if messages.is_empty() {
        muted(b, t, tr("org-chat-empty"));
    }
    for (i, (who, text, when)) in messages.iter().enumerate() {
        let name = if Some(*who) == me {
            tr("shell-landing-me")
        } else {
            social
                .contacts
                .get(who)
                .map(|c| c.name.clone())
                .unwrap_or_else(|| short(who))
        };
        row(b, |r| {
            r.spawn(t.text(name, t.font_sm, FontWeight::BOLD, t.foreground));
            r.spawn(t.text(
                show_when(*when),
                t.font_xs,
                FontWeight::NORMAL,
                t.muted_foreground,
            ));
        });
        b.spawn((
            Name::new(format!("org-chat-{i}")),
            t.paragraph(text.clone(), t.font_base, t.foreground),
        ));
    }
    row(b, |r| {
        r.spawn(
            TextInput::new("org-chat-input")
                .placeholder(tr("chat-placeholder"))
                .width(Val::Px(420.0))
                .build(t),
        );
        r.spawn((
            Button::new("org-chat-send")
                .label(tr("chat-send"))
                .primary()
                .build(t),
            observe(|_: On<Activate>, mut commands: Commands| commands.queue(say)),
        ));
        r.spawn(
            TextInput::new("org-channel-new")
                .placeholder(tr("org-channel-new-placeholder"))
                .width(Val::Px(180.0))
                .build(t),
        );
        r.spawn((
            Button::new("org-channel-open")
                .label(tr("org-channel-open"))
                .build(t),
            observe(|_: On<Activate>, mut commands: Commands| {
                commands.queue(|w: &mut World| {
                    let name = field(w, "org-channel-new")
                        .trim()
                        .trim_start_matches('#')
                        .to_lowercase();
                    if !name.is_empty()
                        && let Some(mut open) = w.get_resource_mut::<OpenOrg>()
                    {
                        open.channel = name;
                    }
                });
            }),
        ));
    });
}

fn say(world: &mut World) {
    let text = field(world, "org-chat-input");
    let Some(channel) = world.get_resource::<OpenOrg>().map(|o| o.channel.clone()) else {
        return;
    };
    if text.trim().is_empty() {
        return;
    }
    write(
        world,
        Op::Say {
            channel,
            text: text.trim().to_owned(),
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn times_round_trip() {
        let millis = parse_when("2026-11-14 08:30").unwrap();
        assert_eq!(show_when(millis), "2026-11-14 08:30");
        assert_eq!(parse_when("tomorrow"), None);
    }

    #[test]
    fn a_bom_finds_stock_materials() {
        let mut org = cadrs_org::Org::default();
        org.materials.insert(
            7,
            cadrs_org::Material {
                name: "mild steel".into(),
                thickness: 10.0,
                sheet: [2500.0, 1250.0],
            },
        );
        let bom = parse_bom("0.5 mild steel 10, 4 M8 bolt", &org);
        assert_eq!(
            bom,
            vec![
                BomLine::Material {
                    material: 7,
                    sheets: 0.5
                },
                BomLine::Bought {
                    name: "M8 bolt".into(),
                    quantity: 4
                }
            ]
        );
    }
}
