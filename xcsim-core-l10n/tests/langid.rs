use xcsim_core_l10n::Lazy;

#[test]
fn check_langid() {

    Lazy::force(&xcsim_core_l10n::LANG_IDENTS);
}
