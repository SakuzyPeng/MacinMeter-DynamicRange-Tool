import i18next from "i18next";
import LanguageDetector from "i18next-browser-languagedetector";
import zhCN from "./locales/zh-CN.json";
import enUS from "./locales/en-US.json";

export type SupportedLanguage = "zh-CN" | "en-US";

export const i18n = i18next.createInstance();

// 初始化 i18next
i18n.use(LanguageDetector).init({
  resources: {
    "zh-CN": { translation: zhCN },
    "en-US": { translation: enUS },
  },
  fallbackLng: "zh-CN",
  supportedLngs: ["zh-CN", "en-US"],
  detection: {
    order: ["localStorage", "navigator"],
    caches: ["localStorage"],
    lookupLocalStorage: "i18nextLng",
  },
  interpolation: {
    escapeValue: false,
  },
});

// 导出常用函数
export const t = i18n.t.bind(i18n);
export const changeLanguage = (lng: SupportedLanguage) =>
  i18n.changeLanguage(lng);
export const getCurrentLanguage = (): SupportedLanguage =>
  (i18n.language as SupportedLanguage) || "zh-CN";

// 更新所有带 data-i18n 属性的元素
export const updateStaticTexts = () => {
  document.querySelectorAll("[data-i18n]").forEach((el) => {
    const key = el.getAttribute("data-i18n");
    if (key) {
      el.textContent = t(key);
    }
  });

  document.querySelectorAll("[data-i18n-placeholder]").forEach((el) => {
    const key = el.getAttribute("data-i18n-placeholder");
    if (key) {
      (el as HTMLInputElement).placeholder = t(key);
    }
  });

  document.querySelectorAll("[data-i18n-title]").forEach((el) => {
    const key = el.getAttribute("data-i18n-title");
    if (key) {
      el.setAttribute("title", t(key));
    }
  });
};

// 语言切换后更新按钮状态
export const updateLanguageButtons = () => {
  const currentLang = getCurrentLanguage();
  const zhBtn = document.getElementById("lang-zh");
  const enBtn = document.getElementById("lang-en");

  if (zhBtn && enBtn) {
    zhBtn.classList.toggle("active", currentLang === "zh-CN");
    enBtn.classList.toggle("active", currentLang === "en-US");
  }
};
