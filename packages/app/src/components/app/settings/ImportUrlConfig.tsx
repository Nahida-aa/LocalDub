import { createSignal, Show } from "solid-js";
import { useMutation } from "@tanstack/solid-query";
import { client } from "#/integrations/fnrpc/client.ts";
import { CardX } from "@repo/ui-solid/custom/card";
import { Button } from "@repo/ui-solid/base/button";
import { Input } from "@repo/ui-solid/base/input";
import { Label } from "@repo/ui-solid/base/label";
import { Loader2 } from "lucide-solid";

export const ImportUrlConfig = () => {
  const [url, setUrl] = createSignal("");
  const [localError, setLocalError] = createSignal("");

  const importMutation = useMutation(() => client.import_config_from_url.mutationOptions());

  async function handleImport() {
    setLocalError("");
    const u = url().trim();
    if (!u) {
      setLocalError("Please enter a URL");
      return;
    }
    try {
      await importMutation.mutateAsync(u);
    } catch (e) {
      const err = e as { message?: string };
      setLocalError(err?.message ?? String(e));
    }
  }

  const errorMessage = () => localError() || importMutation.error?.message || "";

  return (
    <CardX
      title="Import Config from URL"
      description="Fetch a config file from a remote URL and save it into the project folder (the file name is derived from the URL, defaulting to proxy.json)."
      size="sm"
      Footer={
        <div class="flex w-full flex-col gap-2">
          <div class="space-y-1">
            <Label for="cfg-url">URL</Label>
            <Input
              id="cfg-url"
              type="url"
              placeholder="https://example.com/config.json"
              value={url()}
              onInput={(e) => setUrl(e.currentTarget.value)}
            />
          </div>
          <div class="flex flex-wrap items-center gap-3 pt-1">
            <Button size="sm" onClick={handleImport} disabled={importMutation.isPending}>
              {importMutation.isPending && <Loader2 class="animate-spin" />}
              Import & Save
            </Button>
            <Show when={importMutation.data}>
              {(data) => (
                <span class="text-sm text-green-500">
                  Saved to {data().path} ({data().size.toString()} bytes,{" "}
                  {data().is_json ? "JSON" : "text"})
                </span>
              )}
            </Show>
            <Show when={errorMessage()}>
              <span class="text-sm text-red-400">{errorMessage()}</span>
            </Show>
          </div>
        </div>
      }
    />
  );
};
