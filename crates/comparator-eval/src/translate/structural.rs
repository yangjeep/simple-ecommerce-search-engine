use commerce_core::ir::StructuralConstraint;

use crate::solr::case_insensitive_field_regex;

use super::{Translation, TranslationContext};

#[derive(Clone, Copy)]
struct StructuralField<'a> {
    name: &'a str,
    lowercase_companion_suffix: Option<&'a str>,
}

pub(super) fn translate_structural(
    constraint: &StructuralConstraint,
    context: &TranslationContext<'_>,
) -> Translation {
    match constraint {
        StructuralConstraint::Brand(id) => {
            let Some(field) = context.fields.brand else {
                return Translation::NotApplicable;
            };
            match context.names.brand_name(*id) {
                Some(name) => translate_term(context.field(field), name),
                None => Translation::Unresolvable(format!("no brand name registered for {id:?}")),
            }
        }
        StructuralConstraint::BrandAny(ids) => {
            let Some(field) = context.fields.brand else {
                return Translation::NotApplicable;
            };
            translate_any(context.field(field), ids, |id| {
                context.names.brand_name(*id)
            })
        }
        StructuralConstraint::ProductType(id) => {
            let Some(field) = context.fields.product_type else {
                return Translation::NotApplicable;
            };
            match context.names.product_type_name(*id) {
                Some(name) => translate_term(context.field(field), name),
                None => {
                    Translation::Unresolvable(format!("no product_type name registered for {id:?}"))
                }
            }
        }
        StructuralConstraint::ProductTypeAny(ids) => {
            let Some(field) = context.fields.product_type else {
                return Translation::NotApplicable;
            };
            translate_any(context.field(field), ids, |id| {
                context.names.product_type_name(*id)
            })
        }
        StructuralConstraint::Category(id) => {
            let Some(field) = context.fields.category else {
                return Translation::NotApplicable;
            };
            match context.names.category_name(*id) {
                Some(name) => translate_term(context.field(field), name),
                None => {
                    Translation::Unresolvable(format!("no category name registered for {id:?}"))
                }
            }
        }
        StructuralConstraint::PriceUnderCents(cents) => {
            let Some(field) = context.fields.price_cents else {
                return Translation::NotApplicable;
            };
            Translation::Fq(format!("{field}:[* TO {cents}}}"))
        }
        StructuralConstraint::PriceOverCents(cents) => {
            let Some(field) = context.fields.price_cents else {
                return Translation::NotApplicable;
            };
            Translation::Fq(format!("{field}:{{{cents} TO *]"))
        }
    }
}

impl<'a> TranslationContext<'a> {
    fn field(&self, name: &'a str) -> StructuralField<'a> {
        StructuralField {
            name,
            lowercase_companion_suffix: self.lowercase_companion_suffix,
        }
    }
}

fn translate_term(field: StructuralField<'_>, value: &str) -> Translation {
    match field.lowercase_companion_suffix {
        Some(suffix) => Translation::Fq(format!(
            "{}{suffix}:\"{}\"",
            field.name,
            solr_escaped_lowercase_term(value)
        )),
        None => Translation::Fq(format!(
            "{}:/{}/",
            field.name,
            case_insensitive_field_regex(value)
        )),
    }
}

fn translate_any<'a, Id: std::fmt::Debug>(
    field: StructuralField<'_>,
    ids: &[Id],
    resolve: impl Fn(&Id) -> Option<&'a str>,
) -> Translation {
    if ids.is_empty() {
        return Translation::Unresolvable(format!("empty id group for field {}", field.name));
    }
    let mut names = Vec::with_capacity(ids.len());
    for id in ids {
        match resolve(id) {
            Some(name) => names.push(name),
            None => {
                return Translation::Unresolvable(format!(
                    "no name registered for {id:?} (field {}), {}/{} ids resolved so far",
                    field.name,
                    names.len(),
                    ids.len()
                ))
            }
        }
    }
    match field.lowercase_companion_suffix {
        Some(suffix) => {
            let disjunction = names
                .iter()
                .map(|name| format!("\"{}\"", solr_escaped_lowercase_term(name)))
                .collect::<Vec<_>>()
                .join(" OR ");
            Translation::Fq(format!("{}{suffix}:({disjunction})", field.name))
        }
        None => {
            let alternation = names
                .iter()
                .map(|name| case_insensitive_field_regex(name))
                .collect::<Vec<_>>()
                .join("|");
            Translation::Fq(format!("{}:/({alternation})/", field.name))
        }
    }
}

fn solr_escaped_lowercase_term(value: &str) -> String {
    let lowercase = value.to_lowercase();
    let mut escaped = String::with_capacity(lowercase.len());
    for character in lowercase.chars() {
        match character {
            '\\' | '"' => {
                escaped.push('\\');
                escaped.push(character);
            }
            _ => escaped.push(character),
        }
    }
    escaped
}
