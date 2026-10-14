import { For, Match, onMount, Show, Switch } from "solid-js";
import { configStore } from "../../../stores/global/config";
import { showToast } from "../../../stores/global/notifications";
import { APPS, type AppEntry } from "../../../lib/apps";
import type { AppId } from "../../../types/config";
import Button from "../../Button/Button";
import Tooltip from "../../Tooltip/Tooltip";
import { openActivity } from "../../Activity/ActivityPanel";
import AppEntryPoints from "../../Apps/AppEntryPoints";
import {
  canOpenApp,
  openAppSurface,
  switchAppFromSettings,
} from "../../../commands/app-surfaces";
import AiConnectionRows from "../apps/AiConnectionRows";
import ProgramsRows from "../apps/ProgramsRows";
import { SectionLabel } from "../controls";
import {
  activePage,
  closeAppPage,
  isSearching,
  leaveSettings,
  openAppPage,
  returnedFrom,
} from "../state";
import { SwitchRow, ToggleSwitch } from "../SettingsRow";

function appEntry(id: AppId): AppEntry {
  return APPS.find((entry) => entry.id === id)!;
}

async function setAppOn(app: AppId, on: boolean) {
  try {
    await switchAppFromSettings(app, on);
  } catch {
    showToast("Could not save your settings", "error");
  }
}

const aiOn = () => configStore.isAppOn("chat") || configStore.isAppOn("rewrite");

function openProgramActivity() {
  void leaveSettings(() => openActivity());
}

/**
 * Where an app lives once it is on, with Open while it is on (ADR-042
 * section 4). Open closes Settings first, so the surface it shows keeps the
 * focus it takes.
 */
function AppWhere(props: { app: AppEntry }) {
  const on = () => configStore.isAppOn(props.app.id);
  function open() {
    const app = props.app.id;
    void leaveSettings(() => queueMicrotask(() => openAppSurface(app)));
  }
  return (
    <span class="settings-app-where">
      <AppEntryPoints app={props.app} />
      <Show when={on() && canOpenApp(props.app.id)}>
        <Button class="settings-app-open" data-action="open-app" onClick={open}>
          Open
        </Button>
      </Show>
    </span>
  );
}

function OffersRow() {
  return (
    <SwitchRow
      id="apps.offers"
      setting="apps_offers"
      checked={configStore.appOffersOn()}
      onChange={() => configStore.setAppOffersOn(!configStore.appOffersOn())}
    />
  );
}

function AppCard(props: { app: AppEntry }) {
  const on = () => configStore.isAppOn(props.app.id);
  let openRef: HTMLButtonElement | undefined;
  onMount(() => {
    if (returnedFrom() === props.app.id) openRef?.focus();
  });
  return (
    <SwitchRow
      id={props.app.settingId}
      setting={`app_${props.app.id}`}
      checked={on()}
      onChange={() => void setAppOn(props.app.id, !on())}
      footer={<AppWhere app={props.app} />}
      aside={
        <Tooltip label={`Open ${props.app.label}`}>
          <Button
            ref={openRef}
            variant="ghost"
            icon="caret-right"
            iconSize={14}
            data-action={`open-app-page-${props.app.id}`}
            aria-label={`Open ${props.app.label}`}
            onClick={() => openAppPage(props.app.id)}
          />
        </Tooltip>
      }
    />
  );
}

function AppPageRows(props: { app: AppId }) {
  return (
    <Show when={configStore.isAppOn(props.app)}>
      <Switch>
        <Match when={props.app === "chat" || props.app === "rewrite"}>
          <AiConnectionRows withChatModel={props.app === "chat"} />
        </Match>
        <Match when={props.app === "programs"}>
          <ProgramsRows onOpenActivity={openProgramActivity} />
        </Match>
      </Switch>
    </Show>
  );
}

function AppPage(props: { app: AppEntry }) {
  const on = () => configStore.isAppOn(props.app.id);
  let backRef: HTMLButtonElement | undefined;
  onMount(() => backRef?.focus());
  return (
    <div class="settings-app-page" data-app-page={props.app.id}>
      <Button
        ref={backRef}
        variant="ghost"
        class="settings-page-back"
        icon="caret-right"
        iconSize={14}
        data-action="apps-back"
        onClick={closeAppPage}
      >
        Apps
      </Button>
      <div class="settings-app-page-head">
        <h4 class="settings-app-page-title">{props.app.label}</h4>
        <ToggleSwitch
          setting={`app_${props.app.id}_page`}
          label={props.app.label}
          checked={on()}
          onChange={() => void setAppOn(props.app.id, !on())}
        />
      </div>
      <p class="settings-app-page-detail">{props.app.detail}</p>
      <AppWhere app={props.app} />
      <AppPageRows app={props.app.id} />
    </div>
  );
}

export default function AppsSection() {
  return (
    <div data-section="apps">
      <SectionLabel section="apps" />
      <Show
        when={!isSearching()}
        fallback={
          <>
            <For each={APPS}>{(app) => <AppCard app={app} />}</For>
            <OffersRow />
            <Show when={aiOn()}>
              <AiConnectionRows withChatModel={true} />
            </Show>
            <Show when={configStore.isAppOn("programs")}>
              <ProgramsRows onOpenActivity={openProgramActivity} />
            </Show>
          </>
        }
      >
        <Show
          when={activePage()}
          fallback={
            <>
              <div class="settings-app-cards">
                <For each={APPS}>{(app) => <AppCard app={app} />}</For>
              </div>
              <OffersRow />
            </>
          }
        >
          {(page) => <AppPage app={appEntry(page())} />}
        </Show>
      </Show>
    </div>
  );
}
