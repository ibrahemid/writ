import { For, Show, type Accessor } from "solid-js";
import PanelSection from "./PanelSection";
import type { NoteFacts, NoteProperty } from "../../stores/global/note-facts";

interface Props {
  facts: Accessor<NoteFacts>;
}

/**
 * What one frontmatter value reads as: a list of its own entries, or a single
 * value written out.
 */
interface ReadValue {
  items: string[] | null;
  text: string;
}

/**
 * A property's value, as it is written in the note.
 *
 * The index stores it as JSON, and the JSON is what a value that is a number,
 * a date or a nested map has to be read back out of. A value the parser
 * cannot read is shown as the text it is stored as rather than dropped: the
 * file is the truth, and a property missing from this list would say the note
 * does not carry it.
 */
function readValue(property: NoteProperty): ReadValue {
  let parsed: unknown;
  try {
    parsed = JSON.parse(property.value_json);
  } catch {
    return { items: null, text: property.value_json };
  }
  if (Array.isArray(parsed)) {
    return { items: parsed.map((entry) => scalarText(entry)), text: "" };
  }
  return { items: null, text: scalarText(parsed) };
}

function scalarText(value: unknown): string {
  if (typeof value === "string") return value;
  if (value === null || value === undefined) return "";
  if (typeof value === "object") return JSON.stringify(value);
  return String(value);
}

/**
 * The open note's frontmatter, read-only.
 *
 * Nothing here takes a value. The file is what a property is, and a control
 * that wrote one would make the index a second writer of a table whose first
 * writer is a note on disk (ADR-036).
 */
export default function PropertiesSection(props: Props) {
  const properties = () => props.facts().properties;

  return (
    <Show when={properties().length > 0}>
      <PanelSection section="properties" heading="Properties">
        <dl class="right-panel-properties">
          <For each={properties()}>
            {(property) => {
              const value = readValue(property);
              return (
                <div class="right-panel-property">
                  <dt class="right-panel-property-key">{property.key}</dt>
                  <dd class="right-panel-property-value">
                    <Show
                      when={value.items}
                      fallback={<span class="right-panel-property-text">{value.text}</span>}
                    >
                      {(items) => (
                        <For each={items()}>
                          {(item) => <span class="right-panel-pill">{item}</span>}
                        </For>
                      )}
                    </Show>
                  </dd>
                </div>
              );
            }}
          </For>
        </dl>
      </PanelSection>
    </Show>
  );
}
