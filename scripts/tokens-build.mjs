import StyleDictionary from "style-dictionary";
import config from "../design/tokens/config.mjs";

const sd = new StyleDictionary(config);
await sd.buildAllPlatforms();
