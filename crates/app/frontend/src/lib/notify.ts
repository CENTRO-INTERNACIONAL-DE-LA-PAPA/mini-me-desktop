import { isPermissionGranted, requestPermission, sendNotification } from "@tauri-apps/plugin-notification";

let permissionRequested = false;

async function ensurePermission(): Promise<boolean> {
  if (await isPermissionGranted()) return true;
  if (permissionRequested) return false;
  permissionRequested = true;
  const permission = await requestPermission();
  return permission === "granted";
}

function worthInterrupting(): boolean {
  return !document.hasFocus();
}

export async function notifyTurnFinished(body: string) {
  if (!worthInterrupting()) return;
  if (!(await ensurePermission())) return;
  sendNotification({ title: "Mini-Me Desktop", body });
}
