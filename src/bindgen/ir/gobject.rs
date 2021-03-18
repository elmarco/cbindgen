/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

use std::io::Write;

use heck::SnakeCase;
use syn;

use crate::bindgen::config::Config;
use crate::bindgen::declarationtyperesolver::DeclarationTypeResolver;
use crate::bindgen::dependencies::Dependencies;
use crate::bindgen::ir::{
    AnnotationSet, Cfg, Documentation, Item, ItemContainer, Path, Struct, Type,
};
use crate::bindgen::library::Library;
use crate::bindgen::writer::SourceWriter;

#[derive(Debug, Clone)]
pub enum GType {
    Object {
        instance: Option<Type>,
        class: Option<Type>,
        parent_type: Type,
    },
    Interface {
        type_: Type,
    },
}

#[derive(Debug, Clone)]
pub struct GObject {
    pub path: Path,
    pub name: String,
    pub gtype: GType,
    pub cfg: Option<Cfg>,
    pub annotations: AnnotationSet,
    pub documentation: Documentation,
}

impl GObject {
    pub fn load_interface(
        path: &Path,
        mod_cfg: Option<&Cfg>,
        input: &syn::ItemImpl,
    ) -> Result<Self, String> {
        let mut name = None;
        for item in &input.items {
            match item {
                syn::ImplItem::Const(const_) => {
                    let const_name = const_.ident.to_string();
                    if const_name == "NAME" {
                        if let syn::Expr::Lit(syn::ExprLit {
                            lit: syn::Lit::Str(ref lit),
                            ..
                        }) = const_.expr
                        {
                            name = Some(lit.value());
                        }
                    }
                }
                _ => {}
            }
        }

        let type_ = Type::load(&*input.self_ty)?.unwrap();

        Ok(Self::new(
            path.clone(),
            name.unwrap(),
            GType::Interface { type_ },
            Cfg::append(mod_cfg, Cfg::load(&input.attrs)),
            AnnotationSet::load(&input.attrs)?,
            Documentation::load(&input.attrs),
        ))
    }

    pub fn load_object(
        path: &Path,
        mod_cfg: Option<&Cfg>,
        input: &syn::ItemImpl,
    ) -> Result<Self, String> {
        let mut name = None;
        let mut class = None;
        let mut parent_type = None;
        let mut instance = None;
        for item in &input.items {
            match item {
                syn::ImplItem::Type(type_) => {
                    let name = type_.ident.to_string();
                    if name == "Instance" {
                        instance = Type::load(&type_.ty)?;
                    } else if name == "Class" {
                        class = Type::load(&type_.ty)?;
                    } else if name == "ParentType" {
                        parent_type = Type::load(&type_.ty)?;
                    }
                }
                syn::ImplItem::Const(const_) => {
                    let const_name = const_.ident.to_string();
                    if const_name == "NAME" {
                        if let syn::Expr::Lit(syn::ExprLit {
                            lit: syn::Lit::Str(ref lit),
                            ..
                        }) = const_.expr
                        {
                            name = Some(lit.value());
                        }
                    }
                }
                _ => {}
            }
        }

        let gtype = GType::Object {
            instance,
            class,
            parent_type: parent_type.unwrap(),
        };

        Ok(Self::new(
            path.clone(),
            name.unwrap(),
            gtype,
            Cfg::append(mod_cfg, Cfg::load(&input.attrs)),
            AnnotationSet::load(&input.attrs)?,
            Documentation::load(&input.attrs),
        ))
    }

    pub fn new(
        path: Path,
        name: String,
        gtype: GType,
        cfg: Option<Cfg>,
        annotations: AnnotationSet,
        documentation: Documentation,
    ) -> Self {
        Self {
            path,
            name,
            gtype,
            cfg,
            annotations,
            documentation,
        }
    }
}

impl Item for GObject {
    fn path(&self) -> &Path {
        &self.path
    }

    fn add_dependencies(&self, library: &Library, out: &mut Dependencies) {
        match &self.gtype {
            GType::Object {
                parent_type,
                instance,
                class,
            } => {
                parent_type.add_dependencies(library, out);
                if let Some(instance) = instance {
                    instance.add_dependencies(library, out);
                }
                if let Some(class) = class {
                    class.add_dependencies(library, out);
                }
            }
            GType::Interface { type_ } => {
                type_.add_dependencies(library, out);
            }
        }
    }

    fn export_name(&self) -> &str {
        &self.name
    }

    fn cfg(&self) -> Option<&Cfg> {
        self.cfg.as_ref()
    }

    fn annotations(&self) -> &AnnotationSet {
        &self.annotations
    }

    fn annotations_mut(&mut self) -> &mut AnnotationSet {
        &mut self.annotations
    }

    fn container(&self) -> ItemContainer {
        ItemContainer::GObject(self.clone())
    }

    fn rename_for_config(&mut self, _config: &Config) {}

    fn resolve_declaration_types(&mut self, resolver: &DeclarationTypeResolver) {
        match &mut self.gtype {
            GType::Object {
                parent_type,
                instance,
                class,
            } => {
                parent_type.resolve_declaration_types(resolver);
                if let Some(instance) = instance {
                    instance.resolve_declaration_types(resolver);
                }
                if let Some(class) = class {
                    class.resolve_declaration_types(resolver);
                }
            }
            GType::Interface { type_ } => {
                type_.resolve_declaration_types(resolver);
            }
        }
    }
}

impl GObject {
    pub fn write<F: Write>(
        &self,
        _config: &Config,
        out: &mut SourceWriter<F>,
        _associated_to_struct: Option<&Struct>,
    ) {
        let (prefix, name) = match self.gtype {
            GType::Object { .. } => {
                let prefix = self.name.strip_suffix(self.path.name()).unwrap();
                let name = self.name.strip_prefix(prefix).unwrap();
                (prefix, name)
            }
            GType::Interface { .. } => {
                let path_name = self.path.name().strip_suffix("Interface").unwrap();
                let prefix = self.name.strip_suffix(path_name).unwrap();
                let name = self.name.strip_prefix(prefix).unwrap();
                (prefix, name)
            }
        };
        let name_up = name.to_uppercase();
        let prefix_up = prefix.to_uppercase();
        let snake = self.name.to_snake_case();
        let type_up = format!("{}_TYPE_{}", prefix_up, name_up);

        write!(
            out,
            "#define {}                    ({}_get_type())",
            type_up, snake
        );
        out.new_line();
        write!(
            out,
            "#define {}_{}(obj)            (G_TYPE_CHECK_INSTANCE_CAST((obj),{},{}))",
            prefix_up, name_up, type_up, self.name
        );
        out.new_line();
        write!(
            out,
            "#define {}_IS_{}(obj)         (G_TYPE_CHECK_INSTANCE_TYPE((obj),{}))",
            prefix_up, name_up, type_up
        );
        out.new_line();

        match self.gtype {
            GType::Object { .. } => {
                write!(
                    out,
                    "#define {}_{}_CLASS(klass)    (G_TYPE_CHECK_CLASS_CAST((klass),{},{}Class))",
                    prefix_up, name_up, type_up, self.name
                );
                out.new_line();
                write!(
                    out,
                    "#define {}_IS_{}_CLASS(klass) (G_TYPE_CHECK_CLASS_TYPE((klass),{}))",
                    prefix_up, name_up, type_up
                );
                out.new_line();
                write!(
                    out,
                    "#define {}_{}_GET_CLASS(obj)  (G_TYPE_INSTANCE_GET_CLASS((obj),{},{}Class))",
                    prefix_up, name_up, type_up, self.name
                );
            }
            GType::Interface { .. } => {
                write!(
                    out,
                    "#define {}_{}_GET_INTERFACE(obj)  (G_TYPE_INSTANCE_GET_CLASS((obj),{},{}Interface))",
                    prefix_up,
                    name_up,
                    type_up,
                    self.name
                );
            }
        }
    }
}
