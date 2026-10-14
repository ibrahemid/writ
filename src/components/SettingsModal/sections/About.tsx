import { Match, Switch } from "solid-js";
import { executeCommand } from "../../../commands/registry";
import { APP_VERSION } from "../../../lib/app-info";
import { configStore } from "../../../stores/global/config";
import { updateStore } from "../../../stores/global/update";
import Button from "../../Button/Button";
import { SectionLabel, patchConfig } from "../controls";
import { leaveSettings } from "../state";
import { SettingsRow, SwitchRow } from "../SettingsRow";

function UpdateStatus() {
  const phase = () => updateStore.phase();
  const offered = () => {
    const p = phase();
    return p.status === "available" || p.status === "managed" ? p.version : undefined;
  };
  const failure = () => {
    const p = phase();
    return p.status === "failed" ? p.message.trim() || "Could not reach the update server." : undefined;
  };
  return (
    <span class="settings-update-status" data-update-status={phase().status} aria-live="polite">
      <Switch>
        <Match when={phase().status === "checking"}>Checking for updates…</Match>
        <Match when={phase().status === "up_to_date"}>Writ is up to date.</Match>
        <Match when={offered()}>{(version) => `Version ${version()} is available.`}</Match>
        <Match when={phase().status === "downloading"}>Downloading update…</Match>
        <Match when={phase().status === "installing"}>Installing update…</Match>
        <Match when={phase().status === "ready"}>Update installed.</Match>
        <Match when={failure()}>{(message) => message()}</Match>
      </Switch>
    </span>
  );
}

export default function AboutSection() {
  function onAutoCheckToggle() {
    void patchConfig((prev) => ({
      ...prev,
      updater: { ...prev.updater, auto_check: !prev.updater.auto_check },
    }));
  }

  return (
    <div data-section="about">
      <SectionLabel section="about" />
      <SettingsRow id="about.version">
        <span class="settings-inbox-controls">
          <span class="settings-version" data-app-version>
            {APP_VERSION}
          </span>
          <Button
            data-action="whats-new"
            onClick={() => void updateStore.openReleaseNotes(APP_VERSION)}
          >
            What's new
          </Button>
        </span>
      </SettingsRow>
      <SettingsRow id="updates.check_now">
        <span class="settings-inbox-controls">
          <UpdateStatus />
          <Button
            data-action="check-updates-now"
            disabled={updateStore.phase().status === "checking"}
            onClick={() => void updateStore.checkForUpdate()}
          >
            Check now
          </Button>
        </span>
      </SettingsRow>
      <SwitchRow
        id="updates.auto_check"
        setting="updater_auto_check"
        checked={configStore.config().updater.auto_check}
        onChange={onAutoCheckToggle}
      />
      <SettingsRow id="about.docs">
        <Button data-action="open-docs" onClick={() => executeCommand("help.docs")}>
          Open
        </Button>
      </SettingsRow>
      <SettingsRow id="about.licences">
        <Button
          data-action="third-party-notices"
          onClick={() => void leaveSettings(() => executeCommand("app.thirdPartyNotices"))}
        >
          Open
        </Button>
      </SettingsRow>
    </div>
  );
}
