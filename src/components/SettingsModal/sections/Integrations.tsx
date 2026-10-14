import { createSignal, For, onCleanup, onMount, Show } from "solid-js";
import { configStore } from "../../../stores/global/config";
import { inboxStore } from "../../../stores/global/inbox";
import { showToast } from "../../../stores/global/notifications";
import {
  CLI_EXAMPLES,
  copyCliExample,
  fetchCliStatus,
  installCli,
  installFailureMessage,
} from "../../../stores/global/cli";
import type { CliStatus } from "../../../stores/global/cli";
import {
  fetchDefaultAppTypes,
  fetchDefaultAppStatus,
  claimDefaultApp,
} from "../../../stores/global/default-app";
import type { ClaimableType, DefaultAppStatus } from "../../../stores/global/default-app";
import {
  hasSupportedDefaultAppTypes,
  markDefaultAppTypeSupported,
} from "../../../stores/global/default-app-support";
import { logFailure } from "../../../lib/log";
import { FILE_MANAGER_NAME, IS_MAC } from "../../../lib/platform";
import { DEFAULT_APP_SETTING_ID } from "../../../settings";
import { isSettingAvailable } from "../../../settings/availability";
import Button from "../../Button/Button";
import { PathText, SectionLabel, patchConfig } from "../controls";
import { SettingsRow, SwitchRow } from "../SettingsRow";
import { SidebarSectionRow, type SidebarRow } from "./Editor";

/** The row installing the `writ` terminal command, or showing where it is. */
function CliRow() {
  const [isInstallingCli, setIsInstallingCli] = createSignal(false);
  const [status, setStatus] = createSignal<CliStatus | null>(null);
  const installed = () => status()?.installed === true;

  function refreshCliStatus() {
    void fetchCliStatus()
      .then(setStatus)
      .catch(() => setStatus(null));
  }

  onMount(() => {
    refreshCliStatus();
  });

  async function onInstallCli() {
    if (isInstallingCli()) return;
    setIsInstallingCli(true);
    try {
      const result = await installCli();
      showToast(`writ installed at ${result.symlink_path}`, "success");
    } catch (error) {
      showToast(installFailureMessage(error), "error");
      logFailure("the writ command could not be installed");
    } finally {
      refreshCliStatus();
      setIsInstallingCli(false);
    }
  }

  async function onCopyExample(example: string) {
    try {
      await copyCliExample(example);
      showToast("Command copied", "success");
    } catch {
      showToast("Could not copy the command", "error");
    }
  }

  const caution = () => {
    const current = status();
    if (current === null || installed()) return undefined;
    if (current.needs_move) {
      return "Move Writ to your Applications folder and open it from there first.";
    }
    return current.can_install && IS_MAC
      ? "macOS will ask for your password to install this."
      : undefined;
  };

  return (
    <Show when={isSettingAvailable("files.cli")}>
      <SettingsRow
        id="files.cli"
        caution={caution()}
        align={installed() ? "start" : undefined}
      >
        <Show
          when={installed()}
          fallback={
            <Show
              when={status()?.can_install !== false}
              fallback={
                <span class="settings-default-app-status" data-cli-missing>
                  Reinstall Writ to put writ on your PATH.
                </span>
              }
            >
              <Button
                data-action="install-cli"
                disabled={isInstallingCli() || status()?.needs_move === true}
                onClick={() => void onInstallCli()}
              >
                {isInstallingCli() ? "Installing…" : "Install the writ command"}
              </Button>
            </Show>
          }
        >
          <span class="settings-cli" data-cli-installed>
            <PathText class="settings-inbox-path" data-cli-path text={status()?.path ?? ""} />
            <For each={CLI_EXAMPLES}>
              {(example) => (
                <span class="settings-inbox-controls">
                  <span class="settings-cli-example" data-cli-example>
                    {example}
                  </span>
                  <Button
                    data-action="cli-copy-example"
                    aria-label={`Copy ${example}`}
                    onClick={() => void onCopyExample(example)}
                  >
                    Copy
                  </Button>
                </span>
              )}
            </For>
          </span>
        </Show>
      </SettingsRow>
    </Show>
  );
}

const REFRESH_DELAY_MS = 800;

interface DefaultAppItemProps {
  type: ClaimableType;
  status: DefaultAppStatus;
  claiming: boolean;
  onClaim: (typeId: string) => void;
}

/**
 * One file type in the default-app list.
 *
 * The box is checked and disabled once Writ holds the type: Launch Services has
 * no give-it-back call, so an enabled box would offer an action that cannot
 * run. The note under the list says where the handover is undone instead.
 */
function DefaultAppItem(props: DefaultAppItemProps) {
  const extList = () => props.type.exts.map((e) => `.${e}`).join(", ");
  const isDefault = () => props.status.status === "is_default";
  const otherName = () =>
    props.status.status === "other_app" ? (props.status.name ?? null) : null;

  return (
    <label class="settings-file-type">
      <input
        type="checkbox"
        class="settings-file-type-box"
        data-action={`make-default-${props.type.id}`}
        data-default-app-type={props.type.id}
        checked={isDefault()}
        disabled={isDefault() || props.claiming}
        aria-busy={props.claiming}
        onChange={() => props.onClaim(props.type.id)}
      />
      <span class="settings-file-type-text">
        <span class="settings-file-type-name">{props.type.label}</span>{" "}
        <span class="settings-file-type-exts">{extList()}</span>
        <Show when={otherName()}>
          {(name) => <span class="settings-file-type-owner">Now opens in {name()}</span>}
        </Show>
      </span>
    </label>
  );
}

function DefaultAppRow() {
  const [types, setTypes] = createSignal<ClaimableType[]>([]);
  const [statuses, setStatuses] = createSignal<Readonly<Record<string, DefaultAppStatus>>>({});
  const [claimingId, setClaimingId] = createSignal<string | null>(null);
  let refreshTimer: ReturnType<typeof setTimeout> | undefined;

  async function loadStatus(typeId: string) {
    let next: DefaultAppStatus;
    try {
      next = await fetchDefaultAppStatus(typeId);
    } catch {
      // Non-macOS or sandboxed; treat as unsupported.
      next = { status: "unsupported" };
    }
    setStatuses((prev) => ({ ...prev, [typeId]: next }));
    // Keep the shared support registry fresh as each type resolves, so search
    // headers and the command palette never offer a row that will not render.
    markDefaultAppTypeSupported(typeId, next.status !== "unsupported");
  }

  onMount(() => {
    void fetchDefaultAppTypes()
      .then((list) => {
        setTypes(list);
        return Promise.all(list.map((t) => loadStatus(t.id)));
      })
      .catch(() => setTypes([]));
  });

  onCleanup(() => {
    if (refreshTimer !== undefined) clearTimeout(refreshTimer);
  });

  function onClaim(typeId: string) {
    setClaimingId(typeId);
    void claimDefaultApp(typeId)
      .then(() => {
        // Launch Services registers asynchronously — re-query after a short
        // delay and show the handler the OS actually recorded.
        refreshTimer = setTimeout(() => {
          void loadStatus(typeId).finally(() => setClaimingId(null));
        }, REFRESH_DELAY_MS);
      })
      .catch(() => {
        const label = types().find((t) => t.id === typeId)?.label ?? typeId;
        showToast(`Could not make Writ the default for ${label}`, "error");
        // A group spans several UTIs; a mid-loop failure may have claimed some.
        // Re-query so the boxes reflect the real partial state, not a stale one.
        void loadStatus(typeId).finally(() => setClaimingId(null));
      });
  }

  const claimable = () =>
    types().filter((t) => {
      const s = statuses()[t.id];
      return s !== undefined && s.status !== "unsupported";
    });

  // Windows lets no app make itself the default: the person picks Writ in the
  // system's Default apps page, which `set_default_app` opens there.
  const inSystemSettings = () =>
    claimable().some((t) => statuses()[t.id]?.status === "system_settings");

  function onOpenDefaultApps() {
    const first = claimable()[0];
    if (!first) return;
    void claimDefaultApp(first.id).catch(() => {
      showToast("Could not open Default apps", "error");
    });
  }

  // The file-types row is the one that can be missing: the registry answers
  // first, App probes it at startup before Settings can open, and this row
  // re-queries on every visit.
  return (
    <Show when={hasSupportedDefaultAppTypes() || claimable().length > 0}>
      <SettingsRow id={DEFAULT_APP_SETTING_ID}>
        <Show
          when={!inSystemSettings()}
          fallback={
            <Button data-action="open-default-apps" onClick={onOpenDefaultApps}>
              Open Default apps
            </Button>
          }
        >
          <span class="settings-file-types">
            <For each={claimable()}>
              {(t) => (
                <DefaultAppItem
                  type={t}
                  status={statuses()[t.id]!}
                  claiming={claimingId() === t.id}
                  onClaim={onClaim}
                />
              )}
            </For>
            <span class="settings-file-types-note">
              Writ keeps a type until you pick another app in {FILE_MANAGER_NAME}.
            </span>
          </span>
        </Show>
      </SettingsRow>
    </Show>
  );
}

const SIDEBAR_INBOX_ROW: SidebarRow = {
  id: "sidebar.inbox",
  section: "inbox",
  setting: "sidebar_inbox",
};

function WatchedFolderRows() {
  const watchedPath = () => inboxStore.path();

  function onFocusToggle() {
    void patchConfig((prev) => ({ ...prev, inbox: { ...prev.inbox, focus: !prev.inbox.focus } }));
  }

  return (
    <>
      <SettingsRow id="files.inbox_folder">
        <Show
          when={watchedPath()}
          fallback={
            <Button data-action="inbox-watch" onClick={() => void inboxStore.watchFolder()}>
              Choose folder…
            </Button>
          }
        >
          {(path) => (
            <span class="settings-inbox-controls">
              <PathText class="settings-inbox-path" text={path()} />
              <Button data-action="inbox-change" onClick={() => void inboxStore.watchFolder()}>
                Change…
              </Button>
              <Button data-action="inbox-clear" onClick={() => void inboxStore.stopWatching()}>
                Stop watching
              </Button>
            </span>
          )}
        </Show>
      </SettingsRow>
      <SwitchRow
        id="files.inbox_focus"
        setting="inbox_focus"
        checked={configStore.config().inbox.focus}
        onChange={onFocusToggle}
      />
      <SidebarSectionRow row={SIDEBAR_INBOX_ROW} />
    </>
  );
}

export default function IntegrationsSection() {
  return (
    <div data-section="integrations">
      <SectionLabel section="integrations" />
      <CliRow />
      <DefaultAppRow />
      <WatchedFolderRows />
    </div>
  );
}
