import "./style.css";
import init from "solarxy";

console.log("Loading solarxy...");

init()
  .then(() => {
    console.clear();
    console.log("Init ::: solarxy loaded");
  })
  .catch((error: { message: any }) => {
    const msg = String((error && error.message) || error);
    if (
      msg.startsWith(
        "Using exceptions for control flow, don't mind me. This isn't actually an error!",
      )
    ) {
      return;
    }
    console.error("Unexpected init error:", error);
  });
