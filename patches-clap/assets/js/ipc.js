function sendIpc(msg) {
  if (!window.ipc?.postMessage) return false;
  window.ipc.postMessage(JSON.stringify(msg));
  return true;
}

// Tell host we're wired up so it can clear push-dedupe caches and
// resend the current snapshot / tap frame. Ticket 0752.
function postReady(attempts) {
  if (attempts === 300) {
    document.body.textContent = "IPC bridge never attached";
    return;
  }
  if (!sendIpc({ kind: "ready" })) {
    // ipc bridge may attach after script execution; retry briefly.
    setTimeout(() => postReady(attempts + 1), 16);
  }
}
postReady(0);

function postIntent(name, extra) {
  sendIpc({ kind: name, ...extra });
}

api.postIntent = postIntent;
