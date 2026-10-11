import { createFrontendClient } from "@mlx-pilot/plugin-sdk";
const host = createFrontendClient();
document.getElementById("calculate")!.onclick = async () => {
  const result = document.getElementById("result")!;
  try {
    const values = (document.getElementById("values") as HTMLInputElement).value
      .split(",")
      .map((v) => Number(v.trim()));
    if (values.some((v) => !Number.isFinite(v)))
      throw new Error("Use apenas números.");
    result.textContent = JSON.stringify(
      await host.invoke("summarize_numbers", { values }),
      null,
      2,
    );
  } catch (e) {
    result.textContent = String(e);
  }
};
