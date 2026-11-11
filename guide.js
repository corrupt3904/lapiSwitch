(function () {
  var toc = document.getElementById('toc');
  var wide = window.matchMedia('(min-width: 1100px)');
  function syncToc() { toc.open = wide.matches; }
  syncToc();
  if (wide.addEventListener) wide.addEventListener('change', syncToc);
  toc.addEventListener('click', function (e) {
    if (e.target.tagName === 'A' && !wide.matches) toc.open = false;
  });

  var links = {};
  toc.querySelectorAll('a[href^="#"]').forEach(function (a) { links[a.getAttribute('href').slice(1)] = a; });
  if ('IntersectionObserver' in window) {
    var current = null;
    var io = new IntersectionObserver(function (entries) {
      entries.forEach(function (entry) {
        if (entry.isIntersecting) {
          if (current) current.classList.remove('active');
          current = links[entry.target.id];
          if (current) current.classList.add('active');
        }
      });
    }, { rootMargin: '-15% 0px -75% 0px' });
    document.querySelectorAll('main section[id]').forEach(function (s) { io.observe(s); });
  }

  var dlg = document.getElementById('zoom');
  var zimg = dlg.querySelector('img');
  var zcap = dlg.querySelector('p');
  document.querySelectorAll('figure.shot img').forEach(function (img) {
    img.tabIndex = 0;
    img.setAttribute('role', 'button');
    img.setAttribute('aria-label', img.alt + ' — 크게 보기');
    function open() {
      zimg.src = img.src;
      zimg.alt = img.alt;
      var cap = img.closest('figure').querySelector('figcaption');
      zcap.textContent = cap ? cap.textContent : '';
      if (typeof dlg.showModal === 'function') dlg.showModal();
    }
    img.addEventListener('click', open);
    img.addEventListener('keydown', function (e) {
      if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); open(); }
    });
  });
  dlg.addEventListener('click', function () { dlg.close(); });
})();
