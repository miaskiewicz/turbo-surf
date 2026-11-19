window.openAuthModal = function () {
  const modal = document.createElement("div");
  modal.id = "auth-modal";
  modal.setAttribute("role", "dialog");
  modal.style.cssText =
    "position:fixed;inset:0;display:flex;align-items:center;justify-content:center;transition:opacity .2s;opacity:0";
  const paper = document.createElement("div");
  paper.style.cssText = "width:500px;height:200px;overflow-y:auto";
  paper.innerHTML =
    '<button id="modal-action" style="position:relative;width:100px;height:40px">Confirm<span id="modal-ripple" style="position:absolute;inset:0;overflow:hidden;pointer-events:none"></span></button>';
  modal.append(paper);
  document.body.append(modal);
  const probe = document.createElement("div");
  probe.style.cssText = "position:absolute;width:99px;height:99px;overflow:scroll";
  document.body.append(probe);
  const report = {
    viewportOverflow: recordRead(() => innerWidth > document.documentElement.clientWidth),
    scrollbarSize: recordRead(() => probe.offsetWidth - probe.clientWidth),
    padding: recordRead(() => parseInt(getComputedStyle(document.body).paddingRight, 10) || 0),
    reflowTop: recordRead(() => modal.scrollTop),
    reflowLeft: recordRead(() => modal.scrollLeft),
    bodyTop: recordRead(() => document.body.scrollTop),
    rootTop: recordRead(() => document.documentElement.scrollTop),
  };
  probe.remove();
  document.body.style.overflow = "hidden";
  // MUI useModal.handleMounted runs this after ModalManager.mount.
  report.mountTop = recordRead(() => {
    modal.scrollTop = 0;
    return modal.scrollTop;
  });
  modal.style.opacity = "1";
  report.paperWidth = recordRead(() => paper.getBoundingClientRect().width);
  window.modalClicks = 0;
  document.getElementById("modal-action").onmousedown = () => {
    const root = document.getElementById("modal-ripple");
    window.modalRipple = {
      width: recordRead(() => root.getBoundingClientRect().width),
      client: recordRead(() => root.clientWidth),
    };
    const ripple = document.createElement("span");
    ripple.className = "ripple";
    root.append(ripple);
  };
  document.getElementById("modal-action").onclick = (event) => {
    if (event.isTrusted) modalClicks++;
  };
  return report;
};
