//! The organization screen's Members module: who works here and what each may do; adding
//! someone (a contact, or their link), changing a role, removing a member.

use super::*;

pub(super) fn role_key(role: Role) -> &'static str {
    match role {
        Role::Owner => "org-role-owner",
        Role::Admin => "org-role-admin",
        Role::Member => "org-role-member",
    }
}

pub(super) const ROLES: [Role; 3] = [Role::Member, Role::Admin, Role::Owner];

/// The member whose role the dialog changes.
#[derive(Resource)]
pub(super) struct EditingMember(cadrs_social::keys::AccountId);

pub(super) fn members(b: &mut ChildSpawnerCommands, t: &Theme, v: &ModuleView) {
    let org = &v.org;
    page_head(b, t, "org-tab-members", "org-members-about");
    let my_role = v.me.and_then(|me| org.members.get(&me).copied());
    let admin = my_role >= Some(Role::Admin);
    let columns = vec![
        Column::new("who", tr("org-member")).width(220.0),
        Column::new("role", tr("org-member-role")).width(110.0),
        Column::new("devices", tr("org-member-devices")).width(240.0),
        Column::new("actions", "").width(230.0),
    ];
    let rows = org
        .members
        .iter()
        .enumerate()
        .map(|(i, (account, role))| {
            let account = *account;
            let devices = org.devices.of(&account);
            let names: Vec<&str> = devices
                .iter()
                .map(|d| d.name.as_str())
                .filter(|n| !n.is_empty())
                .collect();
            let mut who = v.account_name(&account);
            if Some(account) == v.me {
                who = tr_with("org-member-you", &[("name", &who)]);
            }
            let devices = if names.is_empty() {
                tr_with(
                    "org-member-device-count",
                    &[("count", &devices.len().to_string())],
                )
            } else {
                names.join(", ")
            };
            // An owner is only changed by an owner; nobody removes themselves here.
            let may = admin
                && Some(account) != v.me
                && (*role != Role::Owner || my_role == Some(Role::Owner));
            text_row(
                t,
                format!("org-member-{i}"),
                &columns,
                [who, tr(role_key(*role)), devices],
            )
            .cell(move |c| {
                if !may {
                    return;
                }
                cell_button(
                    c,
                    format!("org-member-role-{i}"),
                    tr("org-member-change-role"),
                    false,
                    move |w| open_member_role(w, account),
                );
                cell_button(
                    c,
                    format!("org-member-remove-{i}"),
                    tr("common-remove"),
                    false,
                    move |w| {
                        ask_remove(
                            w,
                            Op::SetMember {
                                account,
                                role: None,
                            },
                        )
                    },
                );
            })
        })
        .collect();
    table(b, t, "org-members", columns, rows, "org-no-members");
    if !admin {
        return;
    }
    b.spawn((
        Button::new("org-add-member")
            .label(tr("org-add-member"))
            .icon("plus")
            .build(t),
        observe(
            |_: On<Activate>,
             mut commands: Commands,
             theme: Res<Theme>,
             social: Res<crate::social::Social>| {
                let t = theme.clone();
                let tb = theme.clone();
                let people: Vec<String> = people(&social).iter().map(|c| c.name.clone()).collect();
                commands.spawn((
                    Dialog::new("org-member-dialog")
                        .title(tr("org-add-member"))
                        .width(560.0)
                        .body(move |b| {
                            b.spawn(t_text(&tb, "org-member-who"));
                            if !people.is_empty() {
                                b.spawn(choice(&tb, "org-member-contact", people.clone(), 0));
                            }
                            b.spawn(
                                TextInput::new("org-member-link")
                                    .placeholder(tr("org-member-link-placeholder"))
                                    .build(&tb),
                            );
                            b.spawn(t_text(&tb, "org-member-role"));
                            b.spawn(role_strip(&tb, Role::Member));
                        })
                        .footer(|f| dialog_buttons(f, "org-member", add_member))
                        .build(&t),
                    OrgDialog,
                    DespawnOnExit(AppState::Organization),
                ));
            },
        ),
    ));
}

pub(super) fn role_strip(t: &Theme, selected: Role) -> impl Bundle {
    let mut strip = TabStrip::new("org-member-role");
    for role in ROLES {
        strip = strip.tab(tr(role_key(role)));
    }
    strip
        .selected(ROLES.iter().position(|r| *r == selected).unwrap_or(0))
        .build(t)
}

pub(super) fn open_member_role(world: &mut World, account: cadrs_social::keys::AccountId) {
    let Some(role) = world
        .get_resource::<OpenOrg>()
        .and_then(|o| o.session.org().members.get(&account).copied())
    else {
        return;
    };
    world.insert_resource(EditingMember(account));
    let theme = world.resource::<Theme>().clone();
    let tb = theme.clone();
    world.spawn((
        Dialog::new("org-member-role-dialog")
            .title(tr("org-member-change-role"))
            .width(420.0)
            .body(move |b| {
                b.spawn(role_strip(&tb, role));
            })
            .footer(|f| dialog_buttons(f, "org-member-role", set_member_role))
            .build(&theme),
        OrgDialog,
        DespawnOnExit(AppState::Organization),
    ));
    world.flush();
}

pub(super) fn set_member_role(world: &mut World) {
    let Some(EditingMember(account)) = world.remove_resource::<EditingMember>() else {
        return;
    };
    let role = ROLES[strip_selected(world, "org-member-role").min(2)];
    let op = Op::SetMember {
        account,
        role: Some(role),
    };
    if write(world, op) {
        close_dialogs(world);
    } else {
        world.insert_resource(EditingMember(account));
    }
}

/// The people (not organizations) among the contacts.
pub(super) fn people(social: &crate::social::Social) -> Vec<cadrs_social::contact::Card> {
    social
        .contacts
        .cards
        .iter()
        .map(|c| c.value.clone())
        .filter(|c| c.kind == cadrs_social::contact::Kind::Person)
        .collect()
}

pub(super) fn add_member(world: &mut World) {
    crate::social::ensure_loaded(world);
    let link = field(world, "org-member-link");
    let card = if link.trim().is_empty() {
        let i = strip_selected(world, "org-member-contact");
        people(world.resource::<crate::social::Social>())
            .get(i)
            .cloned()
    } else {
        match cadrs_social::contact::from_link(&link) {
            Ok(card) if card.value.kind == cadrs_social::contact::Kind::Person => {
                crate::social::add_contact(world, card.clone());
                Some(card.value)
            }
            _ => None,
        }
    };
    let Some(card) = card else {
        crate::linked::toast(world, tr("org-member-bad-account"));
        return;
    };
    let role = ROLES[strip_selected(world, "org-member-role").min(2)];
    let op = Op::SetMember {
        account: card.account,
        role: Some(role),
    };
    if write(world, op) {
        close_dialogs(world);
        // They join from their account menu; their cadrs then fills the document by syncing.
        crate::org_sync::invite(world, &card);
    }
}
