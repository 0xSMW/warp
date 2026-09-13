#[cfg(any(test, feature = "integration_tests"))]
use session_sharing_protocol::common::Role;
use session_sharing_protocol::common::{Guest, PendingGuest, SessionId, TeamAclData};
use warpui::{Entity, ModelContext, SingletonEntity};

use crate::drive::sharing::SharingAccessLevel;
pub struct SessionPermissionsManager {}

impl SessionPermissionsManager {
    pub(crate) fn new(_ctx: &mut ModelContext<Self>) -> Self {
        Self {}
    }

    // The shared-session cloud transport is disabled in production. Keep these permission-update
    // handlers for the test and integration transports.
    #[cfg(any(test, feature = "integration_tests"))]
    pub(crate) fn updated_guests(
        &mut self,
        ctx: &mut ModelContext<Self>,
        session_id: SessionId,
        guests: Vec<Guest>,
        pending_guests: Vec<PendingGuest>,
    ) {
        ctx.emit(SessionPermissionsEvent::GuestsUpdated {
            session_id,
            guests,
            pending_guests,
        });
    }

    #[cfg(any(test, feature = "integration_tests"))]
    pub(crate) fn updated_link_permissions(
        &mut self,
        session_id: SessionId,
        role: Option<Role>,
        ctx: &mut ModelContext<Self>,
    ) {
        let access_level = role.map(|role| role.into());
        ctx.emit(SessionPermissionsEvent::LinkPermissionsUpdated {
            session_id,
            access_level,
        });
    }

    /// Sets the team ACL for the given session. For now, this assumes that
    /// sessions can have only one team ACL.
    #[cfg(any(test, feature = "integration_tests"))]
    pub(crate) fn updated_team_permissions(
        &mut self,
        session_id: SessionId,
        team_acl: Option<TeamAclData>,
        ctx: &mut ModelContext<Self>,
    ) {
        ctx.emit(SessionPermissionsEvent::TeamPermissionsUpdated {
            session_id,
            team_acl,
        });
    }
}

pub enum SessionPermissionsEvent {
    GuestsUpdated {
        session_id: SessionId,
        guests: Vec<Guest>,
        pending_guests: Vec<PendingGuest>,
    },
    LinkPermissionsUpdated {
        session_id: SessionId,
        access_level: Option<SharingAccessLevel>,
    },
    TeamPermissionsUpdated {
        session_id: SessionId,
        team_acl: Option<TeamAclData>,
    },
}

impl Entity for SessionPermissionsManager {
    type Event = SessionPermissionsEvent;
}

impl SingletonEntity for SessionPermissionsManager {}
