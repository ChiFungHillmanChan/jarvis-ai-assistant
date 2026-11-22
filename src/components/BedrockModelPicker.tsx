import { useEffect, useId, useReducer, type CSSProperties } from "react";
import { useUiLocale } from "../i18n";
import Select from "../ui/Select";
import { OTHER_MODEL, formPickerOptions, formRegionNote, groupLabel, initialPicker, listStatus, otherIdHelp, pickerOptions, pickerReducer, selectValue, type BedrockListState, type PickerPlace } from "../lib/bedrock-models";

/** Each place keeps its own look: Settings its classes and inline styles, the forms theirs. */
const LOOK: Record<PickerPlace, { field: string; help: string }> = {
  settings: { field: "settings-bedrock-model", help: "settings-help" },
  mail: { field: "mail-bedrock-model", help: "mail-hint" },
  setup: { field: "setup-bedrock-model", help: "setup-hint" },
};

type Shared = {
  /** The app's default Bedrock model id, used until the list has arrived. */
  fallback: string;
  listState: BedrockListState;
  onReload: () => void;
};

type Props = Shared & (
  | {
    place: "settings";
    /** The Bedrock row's saved id; empty runs the default model. */
    saved: string;
    /** Saves the id at once; resolves to the refusal, or null. */
    onSave: (model: string) => Promise<string | null>;
    styles: { select: CSSProperties; button: CSSProperties; input: CSSProperties };
  }
  | {
    place: "mail" | "setup";
    /** The form's id. */
    value: string;
    /** The id saved in mail settings. */
    stored: string;
    /** Puts the id in the form; the form's own button saves it. */
    onChange: (model: string) => void;
  }
);

/**
 * The Bedrock model, picked by name from the list the page loaded
 * (lib/bedrock-models.ts): AWS's list for the saved region, or the built-in
 * list, with a line saying which. "Other (enter an ID)" shows the ID field.
 * An id saved earlier that the list does not hold stays the current choice.
 * In Settings > Models a listed model or the default is saved at once, and
 * a typed id with "Save model". In the Mail dialog and the setup guide a
 * choice changes the form only, a model is required, the form's own button
 * saves it, and a line (or, under "Other", the ID field's help) says that a
 * us., eu. or apac. profile is sent for the saved region
 * (mail/analysis.rs analyze_with_endpoints, as chat).
 */
export default function BedrockModelPicker(props: Props) {
  const { place, fallback, listState, onReload } = props;
  const { text } = useUiLocale();
  const form = props.place !== "settings";
  const look = props.place === "settings" ? props.styles : undefined;
  /**
   * The id the picker holds, which the reducer calls saved: the row's saved
   * id in Settings, the form's id in the forms.
   */
  const saved = props.place === "settings" ? props.saved : props.value;
  const [state, dispatch] = useReducer(pickerReducer, saved, initialPicker);
  useEffect(() => { dispatch({ type: "synced", saved }); }, [saved]);
  const statusId = useId();
  const list = listState.status === "ready" ? listState.list : null;
  const options = props.place === "settings"
    ? pickerOptions(list, props.saved, fallback, text)
    : formPickerOptions(list, state.other ? null : props.value, props.stored, fallback, text);
  const busy = state.pending !== null;
  const changed = state.draft.trim() !== saved;
  /** Settings saves the id at once; a form only takes it, and its own button saves. */
  async function save(value: string) {
    if (props.place !== "settings") {
      props.onChange(value);
      return;
    }
    const { onSave } = props;
    dispatch({ type: "saving", value });
    dispatch({ type: "saved", error: await onSave(value) });
  }
  function choose(value: string) {
    dispatch({ type: "choose", value, saved });
    if (value !== OTHER_MODEL && value !== saved) void save(value);
  }
  /** In a form the ID field's text is the form's id as it is typed. */
  function type(draft: string) {
    dispatch({ type: "type", draft });
    if (props.place !== "settings") props.onChange(draft.trim());
  }
  return (
    <div className={LOOK[place].field}>
      <Select label={text("Bedrock model", "Bedrock 模型")} showLabel value={selectValue(state, saved)} disabled={busy} describedBy={statusId} onChange={choose} required={form}
        options={options.map((option) => ({ value: option.value, label: option.label, disabled: option.disabled, group: groupLabel(option.group, text) ?? undefined }))} />
      <button type="button" disabled={listState.status === "loading" || busy} onClick={onReload} style={look?.button}>
        {listState.status === "idle" ? text("Load the list", "載入清單") : text("Load the list again", "重新載入清單")}
      </button>
      <p id={statusId} role="status" className={LOOK[place].help}>{listStatus(listState, text, place)}</p>
      {props.place !== "settings" && !state.other && <p className={LOOK[place].help}>{formRegionNote(props.place, text)}</p>}
      {state.other && (<>
        <label>
          <span>{text("Bedrock model ID", "Bedrock 模型 ID")}</span>
          <input value={state.draft} placeholder={form ? undefined : fallback} spellCheck={false} autoCapitalize="off" autoCorrect="off" disabled={busy} required={form}
            onChange={(e) => type(e.target.value)}
            onKeyDown={(e) => { if (!form && e.key === "Enter" && changed && !busy) void save(state.draft.trim()); }}
            style={look && { ...look.input, fontFamily: "var(--font-mono)" }} />
        </label>
        {!form && <button disabled={!changed || busy} onClick={() => void save(state.draft.trim())} style={look?.button}>{text("Save model", "儲存模型")}</button>}
      </>)}
      {/* Only Settings saves, so only Settings can be refused. */}
      {state.error && <p role="alert" className="settings-bedrock-model-error">{state.error}</p>}
      {state.other && <p className={LOOK[place].help}>{otherIdHelp(place, fallback, text)}</p>}
    </div>
  );
}
