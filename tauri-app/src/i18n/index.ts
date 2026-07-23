import enUS from "./locales/en-US.json";
import zhCN from "./locales/zh-CN.json";

export type SupportedLanguage = "zh-CN" | "en-US";
type Values = Record<string, string | number>;

const STORAGE_KEY = "macinmeter-language";
const resources: Record<SupportedLanguage, Record<string, string>> = {
  "zh-CN": zhCN,
  "en-US": enUS,
};

const detectedLanguage = (): SupportedLanguage => {
  const stored = localStorage.getItem(STORAGE_KEY);
  if (stored === "zh-CN" || stored === "en-US") return stored;
  return navigator.language.toLowerCase().startsWith("zh") ? "zh-CN" : "en-US";
};

let language = detectedLanguage();

export const getCurrentLanguage = (): SupportedLanguage => language;

export const changeLanguage = (next: SupportedLanguage): void => {
  language = next;
  localStorage.setItem(STORAGE_KEY, next);
  document.documentElement.lang = next;
};

export const t = (key: string, values: Values = {}): string => {
  const template = resources[language][key] ?? resources["zh-CN"][key] ?? key;
  return Object.entries(values).reduce(
    (text, [name, value]) => text.split(`{{${name}}}`).join(String(value)),
    template,
  );
};

export const updateStaticTexts = (): void => {
  document.documentElement.lang = language;
  document.querySelectorAll<HTMLElement>("[data-i18n]").forEach((element) => {
    const key = element.dataset.i18n;
    if (key) element.textContent = t(key);
  });
  document
    .querySelectorAll<HTMLInputElement>("[data-i18n-placeholder]")
    .forEach((element) => {
      const key = element.dataset.i18nPlaceholder;
      if (key) element.placeholder = t(key);
    });
  document.querySelectorAll<HTMLElement>("[data-i18n-title]").forEach((element) => {
    const key = element.dataset.i18nTitle;
    if (key) element.title = t(key);
  });
  document
    .querySelectorAll<HTMLElement>("[data-i18n-aria-label]")
    .forEach((element) => {
      const key = element.dataset.i18nAriaLabel;
      if (key) element.setAttribute("aria-label", t(key));
    });
};

export const updateLanguageButtons = (): void => {
  document
    .querySelector("#lang-zh")
    ?.classList.toggle("active", language === "zh-CN");
  document
    .querySelector("#lang-en")
    ?.classList.toggle("active", language === "en-US");
};
