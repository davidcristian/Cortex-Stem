//! Settings changes: the commands that replace or edit the document, and
//! `Inner::mutate_settings`, the one way the document changes.

use stem_core::{EngineInput, Settings};

use crate::{DockService, Inner, ServiceResult, WorkerMsg};

impl DockService {
    /// Validate, persist, and apply new settings. Returns the validated copy.
    ///
    /// The pinned and hidden apps are kept as they are, whatever `settings`
    /// says (`docs/ipc/`, v1.26): only their own commands (`pin_app`,
    /// `pin_target`, `unpin_app`, `reorder_pinned`, `hide_app`,
    /// `unhide_app`) and [`Self::reset_settings`] change them, so a document
    /// the settings page built before a pin or a hide from the dock cannot
    /// undo it when it arrives after them.
    pub fn update_settings(&self, settings: Settings) -> ServiceResult<Settings> {
        self.inner.mutate_settings(|current| {
            let pinned = std::mem::take(&mut current.pinned);
            let hidden_apps = std::mem::take(&mut current.hidden_apps);
            *current = settings;
            current.pinned = pinned;
            current.hidden_apps = hidden_apps;
            true
        })
    }

    /// Restore and persist the defaults, the pinned and hidden apps
    /// included (they are empty there). Returns the validated copy.
    pub fn reset_settings(&self) -> ServiceResult<Settings> {
        self.inner.mutate_settings(|current| {
            *current = Settings::default();
            true
        })
    }

    /// Change the settings in place, under the lock every settings change
    /// takes, then validate, persist and apply them; returns the validated
    /// copy. For an edit of one field (the tray's click-through toggle) that
    /// must not write back a copy read before another change landed.
    pub fn modify_settings(&self, edit: impl FnOnce(&mut Settings)) -> ServiceResult<Settings> {
        self.inner.mutate_settings(|current| {
            edit(current);
            true
        })
    }
}

impl Inner {
    /// The one way settings change. `mutate` edits a copy of the current
    /// document and says whether it changed anything; if so the copy is
    /// migrated, validated, persisted and handed to the engine (which then
    /// notifies `Settings` listeners). All of it happens under the settings
    /// lock, so mutations and saves are serialised: a second call always
    /// sees the first one's result, and `settings.json.tmp` has one writer.
    /// Returns the document as it stands afterwards.
    ///
    /// A failed save leaves the authoritative copy untouched (the caller
    /// gets the store error), so memory and disk never disagree.
    pub(crate) fn mutate_settings(
        &self,
        mutate: impl FnOnce(&mut Settings) -> bool,
    ) -> ServiceResult<Settings> {
        let mut current = self.settings.lock();
        let mut next = current.clone();
        if !mutate(&mut next) {
            return Ok(next);
        }
        next.migrate();
        next.validate();
        self.store.save(&next.to_json_pretty())?;
        *current = next.clone();
        let _ = self
            .worker_tx
            .send(WorkerMsg::Input(EngineInput::SettingsChanged(next.clone())));
        Ok(next)
    }
}
