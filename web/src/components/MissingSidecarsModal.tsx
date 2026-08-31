// The missing-sidecars import prompt (multi-file model preflight). The
// browser can only read files the user hands it, so when a picked model
// references companions that are not staged (a glTF's external .bin and
// textures, an OBJ's .mtl), this dialog lists them BEFORE the import node
// cooks, instead of letting the parse fail with "missing external asset".
// Adding files stages them and re-diffs; completing runs the deferred
// import action (the widget's setParam or the drop flow's node creation).

import { useRef } from "react";
import { completeModelImport, dispatch, stagedManifestNames, stageFile } from "../engine/session";
import { hasMissing, missingSidecars } from "../engine/sidecars";
import { useUi } from "../store/ui";
import { Modal } from "./Modal";

export function MissingSidecarsModal() {
  const prompt = useUi((s) => s.sidecarPrompt);
  const inputRef = useRef<HTMLInputElement>(null);

  if (!prompt) return null;

  const close = () => useUi.getState().setSidecarPrompt(null);

  const complete = () => {
    if (prompt.complete.kind === "setParam") {
      const { ctx, node, key } = prompt.complete;
      dispatch({
        type: "setParam",
        ctx,
        node,
        key,
        value: { kind: "literal", type: "asset", value: prompt.primaryHash },
      });
    } else {
      completeModelImport(prompt.primaryHash, prompt.primaryName);
    }
    close();
  };

  const onAddFiles = async (files: File[]) => {
    for (const file of files) await stageFile(file);
    const missing = missingSidecars(prompt.missing, stagedManifestNames());
    if (hasMissing(missing)) {
      useUi.getState().setSidecarPrompt({ ...prompt, missing });
    } else {
      complete();
    }
  };

  const required = prompt.missing.required;
  const optional = prompt.missing.optional;

  return (
    <Modal
      title="Missing companion files"
      onClose={close}
      footer={
        <div className="modal-actions">
          <button className="btn" onClick={close}>
            Cancel
          </button>
          <button className="btn" onClick={complete}>
            Import Anyway
          </button>
          <button className="btn primary" onClick={() => inputRef.current?.click()}>
            Add Files…
          </button>
          <input
            ref={inputRef}
            type="file"
            multiple
            style={{ display: "none" }}
            onChange={(e) => {
              const files = Array.from(e.target.files ?? []);
              if (files.length > 0) void onAddFiles(files);
              e.target.value = "";
            }}
          />
        </div>
      }
    >
        <p className="sidecar-intro">
          <strong>{prompt.primaryName}</strong> references files that were not imported. The
          browser can only read files you select, so add them below. Tip: dragging the
          model's whole folder onto the app imports everything at once.
        </p>
        {required.length > 0 && (
          <div className="sidecar-group">
            <div className="sidecar-group-title">Required (the import fails without these)</div>
            <ul className="sidecar-list">
              {required.map((n) => (
                <li key={n}>{n}</li>
              ))}
            </ul>
          </div>
        )}
        {optional.length > 0 && (
          <div className="sidecar-group">
            <div className="sidecar-group-title">
              Optional (materials and textures; the model loads without them,
              untextured)
            </div>
            <ul className="sidecar-list">
              {optional.map((n) => (
                <li key={n}>{n}</li>
              ))}
            </ul>
          </div>
        )}
    </Modal>
  );
}
