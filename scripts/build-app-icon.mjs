import { ICON_BUILD_ERRORS, runAppIconBuild } from "./app-icon/cli.mjs";

try {
  runAppIconBuild(process.argv.slice(2));
} catch (error) {
  if (!ICON_BUILD_ERRORS.some((ErrorClass) => error instanceof ErrorClass)) throw error;
  console.error(`${error.name}: ${error.message}`);
  process.exitCode = 1;
}
