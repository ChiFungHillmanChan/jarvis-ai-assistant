// The Bedrock model picker's list for the Mail dialog and the setup guide
// (lib/bedrock-models.ts, components/BedrockModelPicker.tsx), asked for by
// the same rule as in Settings > Advanced > Models: only while the picker is on screen,
// only after someone has clicked or pressed a key in this window
// (lib/user-activity.ts), once per opening, and again when `reload` is
// called (the "Load the list again" button, and the setup guide after it has
// saved AWS credentials). Closing the picker forgets the list and drops a
// reply still on its way. The list is kept in memory only.
import { useCallback, useEffect, useRef, useState } from "react";
import { listBedrockModels } from "../lib/commands.ts";
import type { BedrockListState } from "../lib/bedrock-models.ts";
import { useUserHasActed } from "../lib/user-activity.ts";

export function useBedrockModelList(onScreen: boolean): { list: BedrockListState; reload: () => void } {
  const userActed = useUserHasActed();
  const [list, setList] = useState<BedrockListState>({ status: "idle" });
  const [ask, setAsk] = useState(0);
  const asked = useRef<number | null>(null);
  const request = useRef(0);
  useEffect(() => {
    if (!onScreen) {
      asked.current = null;
      request.current += 1;
      setList({ status: "idle" });
      return;
    }
    if (!userActed || asked.current === ask) return;
    asked.current = ask;
    const current = ++request.current;
    setList({ status: "loading" });
    listBedrockModels()
      .then((value) => { if (request.current === current) setList({ status: "ready", list: value }); })
      .catch(() => { if (request.current === current) setList({ status: "error" }); });
  }, [onScreen, userActed, ask]);
  const reload = useCallback(() => setAsk((n) => n + 1), []);
  return { list, reload };
}
