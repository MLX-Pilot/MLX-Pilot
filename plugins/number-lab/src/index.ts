import { definePlugin, defineTool } from "@mlx-pilot/plugin-sdk";
export default definePlugin({
  id: "number_lab",
  name: "number-lab",
  version: "1.1.0",
  frontend: {
    html: "src/index.html",
    script: "src/frontend.ts",
    style: "src/frontend.css",
  },
  tools: [
    defineTool({
      name: "summarize_numbers",
      description: "Calculate the count and sum of supplied numbers.",
      parameters: {
        type: "object",
        properties: {
          values: { type: "array", items: { type: "number" }, maxItems: 1000 },
        },
        required: ["values"],
        additionalProperties: false,
      },
      execute({ values }: { values: number[] }) {
        return { count: values.length, sum: values.reduce((a, b) => a + b, 0) };
      },
    }),
  ],
});
