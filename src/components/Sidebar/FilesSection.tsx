import { Show } from "solid-js";
import { useWindow } from "../WindowProvider/WindowProvider";
import { workspaceStore } from "../../stores/global/workspace";
import { sidebarFolderStore, type FolderTree } from "../../stores/global/sidebar-folder";
import { basename } from "../../lib/path";
import Icon from "../Icon/Icon";
import Tooltip from "../Tooltip/Tooltip";
import FileTree from "./FileTree";
import SidebarEmpty from "./SidebarEmpty";
import SidebarSection from "./SidebarSection";
import "./FilesSection.css";

/**
 * The sidebar's folder: the one opened by hand while there is one, the notes
 * folder otherwise (NV-01). The heading carries the tag the tree is filtered
 * by, so a filter stays visible with the Tags section folded (NV-08).
 */
export default function FilesSection() {
  const win = useWindow();

  // The tag filtering this tree, which is the selected tag only while the
  // tree holds the files tags name.
  const filterTag = () =>
    sidebarFolderStore.showsNotes() ? win.sidebar.selectedTag() : null;

  return (
    <Show when={sidebarFolderStore.tree()} keyed>
      {(folder: FolderTree) => {
        const isNotes = folder !== workspaceStore;
        const root = () => folder.root() ?? "";
        // An empty notes folder gets the empty state, which says so and offers
        // the two ways to fill it. Read failures and an unread folder are the
        // tree's to show.
        const isEmptyNotesFolder = () => {
          if (!isNotes || filterTag() !== null) return false;
          const listing = folder.entriesFor(root());
          return listing !== undefined && listing.length === 0 && !folder.errorFor?.(root());
        };
        return (
          <SidebarSection
            id="folder"
            heading={basename(root())}
            class="files-section"
            action={
              <>
                <Show when={filterTag()}>
                  {(tag) => (
                    <span class="files-tag-chip">
                      <span class="files-tag-chip-name">#{tag()}</span>
                      <Tooltip label="Show all files">
                        <button
                          type="button"
                          class="files-tag-chip-clear"
                          aria-label="Show all files"
                          onClick={() => win.sidebar.selectTag(null)}
                        >
                          <Icon name="x" size={12} />
                        </button>
                      </Tooltip>
                    </span>
                  )}
                </Show>
                <Show when={!isNotes}>
                  <Tooltip label="Close folder">
                    <button
                      type="button"
                      class="sidebar-section-action files-section-action"
                      aria-label="Close folder"
                      onClick={() => void workspaceStore.closeFolder()}
                    >
                      <Icon name="x" size={16} />
                    </button>
                  </Tooltip>
                </Show>
              </>
            }
          >
            <Show when={!isEmptyNotesFolder()} fallback={<SidebarEmpty />}>
              <FileTree folder={folder} />
            </Show>
          </SidebarSection>
        );
      }}
    </Show>
  );
}
