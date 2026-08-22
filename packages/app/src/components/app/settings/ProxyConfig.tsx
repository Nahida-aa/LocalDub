import { createSignal } from "solid-js";
import { useMutation } from "@tanstack/solid-query";
import { Button } from "@repo/ui-solid/base/button";
import { Input } from "@repo/ui-solid/base/input";
import { DialogFooter } from "@repo/ui-solid/base/dialog";
import { CardX } from "@repo/ui-solid/custom/card";
import { Modal } from "@repo/ui-solid/custom/modal/modal";
import { toastError, toastSuccess } from "@repo/ui-solid/custom/toast";
import { fnrpc } from "#/integrations/fnrpc/client.ts";

export function ProxyConfig() {
  const [url, setUrl] = createSignal("");
  const [confirmOpen, setConfirmOpen] = createSignal(false);

  const importM = useMutation(() => ({
    mutationFn: (proxyUrl: string) => fnrpc.import_config_from_url(proxyUrl),
    onSuccess: (res) => {
      toastSuccess(
        "Proxy config saved",
        `Saved to ${res.path} (${res.size} bytes, via ${res.ua}${res.decoded ? ", base64 decoded" : ""})`,
      );
      setConfirmOpen(false);
    },
    onError: (e) => toastError(e),
  }));

  const onConfirm = () => {
    const trimmed = url().trim();
    if (!trimmed) return;
    importM.mutate(trimmed);
  };

  return (
    <div class="space-y-4">
      <h2>Proxy</h2>
      <CardX
        title="Proxy URL"
        description="配置代理url用于网络请求"
        size="sm"
        Footer={
          <div class="flex w-full gap-2">
            <Input
              placeholder="http://127.0.0.1:7890"
              value={url()}
              onInput={(e) => setUrl(e.currentTarget.value)}
            />
            <Button
              class="font-medium bg-green-400 shrink-0 disabled:opacity-40"
              disabled={!url().trim() || importM.isPending}
              onClick={() => setConfirmOpen(true)}
            >
              Apply
            </Button>
          </div>
        }
      />
      <Modal
        open={confirmOpen()}
        onOpenChange={setConfirmOpen}
        title="Confirm Proxy"
        description={`Use proxy URL: ${url()}`}
        showCloseButton={false}
        size="sm"
      >
        <DialogFooter class="gap-2 pt-2">
          <Button
            variant="outline"
            disabled={importM.isPending}
            onClick={() => setConfirmOpen(false)}
          >
            Cancel
          </Button>
          <Button class="bg-green-400" disabled={importM.isPending} onClick={onConfirm}>
            {importM.isPending ? "Loading..." : "OK"}
          </Button>
        </DialogFooter>
      </Modal>
    </div>
  );
}
