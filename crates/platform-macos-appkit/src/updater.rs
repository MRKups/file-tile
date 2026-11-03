//! Runtime-loaded Sparkle 2 updater adapter (`SPUStandardUpdaterController`).
//!
//! Dynamically loads `Contents/Frameworks/Sparkle.framework` inside packaged `.app` bundles
//! so `cargo run`, `cargo test`, and unkeyed builds operate without hard link dependencies
//! or startup error dialogs.

use std::marker::PhantomData;
use std::path::Path;

use objc2::rc::{Allocated, Retained};
use objc2::runtime::{AnyObject, Bool};
use objc2::{MainThreadMarker, msg_send};
use objc2_foundation::{NSBundle, NSObject, NSString};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdaterStatus {
    /// Packaged `.app` with `Sparkle.framework`, `SUFeedURL`, and `SUPublicEDKey` active.
    Ready,
    /// Packaged `.app` with `Sparkle.framework`, awaiting `SUPublicEDKey` configuration.
    AwaitingPublicKey,
    /// Unbundled binary (`cargo run` / `task run`) or bundle without `Sparkle.framework`.
    Unbundled,
    /// `Sparkle.framework` was present but could not be initialized.
    Unavailable(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdaterSnapshot {
    pub status: UpdaterStatus,
    pub can_check: bool,
    pub automatic_checks: bool,
}

/// Main-thread Sparkle 2 controller handle.
pub struct ApplicationUpdater {
    controller: Option<Retained<NSObject>>,
    status: UpdaterStatus,
    _main_thread_only: PhantomData<*mut ()>,
}

impl ApplicationUpdater {
    /// Inspects the running bundle and starts `SPUStandardUpdaterController` when configured.
    pub fn install() -> Self {
        if MainThreadMarker::new().is_none() {
            return Self::inactive(UpdaterStatus::Unavailable(
                "Sparkle updater installation requires the main thread.".to_string(),
            ));
        }
        let main_bundle = NSBundle::mainBundle();
        let bundle_path = main_bundle.bundlePath().to_string();
        if !bundle_path.ends_with(".app") {
            return Self::inactive(UpdaterStatus::Unbundled);
        }
        let Some(frameworks_dir) = main_bundle.privateFrameworksPath() else {
            return Self::inactive(UpdaterStatus::Unbundled);
        };
        let sparkle_path = Path::new(&frameworks_dir.to_string()).join("Sparkle.framework");
        if !sparkle_path.exists() {
            return Self::inactive(UpdaterStatus::Unbundled);
        }
        if info_string(&main_bundle, "SUFeedURL").is_none() {
            return Self::inactive(UpdaterStatus::Unavailable(
                "SUFeedURL is missing from Info.plist.".to_string(),
            ));
        }
        if info_string(&main_bundle, "SUPublicEDKey").is_none() {
            return Self::inactive(UpdaterStatus::AwaitingPublicKey);
        }

        let Some(sparkle_path_str) = sparkle_path.to_str() else {
            return Self::inactive(UpdaterStatus::Unavailable(
                "Sparkle.framework path is not valid UTF-8.".to_string(),
            ));
        };
        let path_ns = NSString::from_str(sparkle_path_str);
        let Some(framework) = NSBundle::bundleWithPath(&path_ns) else {
            return Self::inactive(UpdaterStatus::Unavailable(
                "Cannot open Sparkle.framework bundle.".to_string(),
            ));
        };
        // SAFETY: `framework` points to `Contents/Frameworks/Sparkle.framework` inside the
        // signed `.app` bundle on the main thread.
        let loaded = unsafe { framework.load() };
        if !loaded {
            return Self::inactive(UpdaterStatus::Unavailable(
                "Cannot load Sparkle.framework into the process.".to_string(),
            ));
        }
        let class_name = NSString::from_str("SPUStandardUpdaterController");
        let Some(cls) = framework.classNamed(&class_name) else {
            return Self::inactive(UpdaterStatus::Unavailable(
                "SPUStandardUpdaterController class not found in Sparkle.framework.".to_string(),
            ));
        };
        // SAFETY: `SPUStandardUpdaterController` inherits from `NSObject` and responds to
        // `initWithStartingUpdater:updaterDelegate:userDriverDelegate:` on the main thread.
        let controller: Option<Retained<NSObject>> = unsafe {
            let allocated: Allocated<NSObject> = msg_send![cls, alloc];
            let nil_delegate: Option<&AnyObject> = None;
            msg_send![
                allocated,
                initWithStartingUpdater: Bool::YES,
                updaterDelegate: nil_delegate,
                userDriverDelegate: nil_delegate,
            ]
        };
        let Some(controller) = controller else {
            return Self::inactive(UpdaterStatus::Unavailable(
                "Sparkle updater controller failed to initialize.".to_string(),
            ));
        };
        Self {
            controller: Some(controller),
            status: UpdaterStatus::Ready,
            _main_thread_only: PhantomData,
        }
    }

    fn inactive(status: UpdaterStatus) -> Self {
        Self {
            controller: None,
            status,
            _main_thread_only: PhantomData,
        }
    }

    fn updater(&self) -> Option<Retained<NSObject>> {
        MainThreadMarker::new()?;
        let controller = self.controller.as_ref()?;
        // SAFETY: `controller` is a valid `SPUStandardUpdaterController` whose `-updater`
        // property returns `SPUUpdater *` on the main thread.
        unsafe { msg_send![controller, updater] }
    }

    /// Reads current updater readiness and automatic-check state.
    pub fn snapshot(&self) -> UpdaterSnapshot {
        let Some(updater) = self.updater() else {
            return UpdaterSnapshot {
                status: self.status.clone(),
                can_check: false,
                automatic_checks: false,
            };
        };
        // SAFETY: `updater` is a valid `SPUUpdater` queried on the main thread.
        let (can_check, automatic): (Bool, Bool) = unsafe {
            let can_check: Bool = msg_send![&updater, canCheckForUpdates];
            let automatic: Bool = msg_send![&updater, automaticallyChecksForUpdates];
            (can_check, automatic)
        };
        UpdaterSnapshot {
            status: self.status.clone(),
            can_check: can_check.as_bool(),
            automatic_checks: automatic.as_bool(),
        }
    }

    fn status_error(&self) -> &'static str {
        match self.status {
            UpdaterStatus::Unbundled => "Available in packaged .app builds.",
            UpdaterStatus::AwaitingPublicKey => {
                "Set public_ed_key in packaging/sparkle.toml to activate updates."
            }
            UpdaterStatus::Unavailable(_) | UpdaterStatus::Ready => {
                "Sparkle updater is unavailable in this build."
            }
        }
    }

    /// Triggers Sparkle's interactive update check window.
    pub fn check_for_updates(&self) -> Result<(), &'static str> {
        if MainThreadMarker::new().is_none() {
            return Err("Update checks require the main thread.");
        }
        let Some(controller) = self.controller.as_ref() else {
            return Err(self.status_error());
        };
        // SAFETY: `controller` is a valid `SPUStandardUpdaterController` receiving
        // `-checkForUpdates:` with a nil sender on the main thread.
        unsafe {
            let sender: Option<&AnyObject> = None;
            let (): () = msg_send![controller, checkForUpdates: sender];
        }
        Ok(())
    }

    /// Sets Sparkle's `automaticallyChecksForUpdates` preference (`NSUserDefaults`-backed).
    pub fn set_automatic_checks(&self, enabled: bool) -> Result<bool, &'static str> {
        let Some(updater) = self.updater() else {
            return Err(self.status_error());
        };
        // SAFETY: `updater` is a valid `SPUUpdater` mutated and queried on the main thread.
        let current: Bool = unsafe {
            let (): () = msg_send![&updater, setAutomaticallyChecksForUpdates: Bool::new(enabled)];
            msg_send![&updater, automaticallyChecksForUpdates]
        };
        Ok(current.as_bool())
    }
}

fn info_string(bundle: &NSBundle, key: &str) -> Option<String> {
    let key_ns = NSString::from_str(key);
    let value = bundle.objectForInfoDictionaryKey(&key_ns)?;
    let text = value.downcast_ref::<NSString>()?.to_string();
    let trimmed = text.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_owned())
}
