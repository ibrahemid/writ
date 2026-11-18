import { OG_IMAGE_ERRORS, runOgImageBuild } from "./og-image/cli.mjs";

try {
  await runOgImageBuild(process.argv.slice(2));
} catch (error) {
  if (!OG_IMAGE_ERRORS.some((ErrorClass) => error instanceof ErrorClass)) throw error;
  console.error(`${error.name}: ${error.message}`);
  process.exitCode = 1;
}
