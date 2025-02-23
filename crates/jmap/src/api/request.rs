/*
 * Copyright (c) 2023 Stalwart Labs Ltd.
 *
 * This file is part of Stalwart Mail Server.
 *
 * This program is free software: you can redistribute it and/or modify
 * it under the terms of the GNU Affero General Public License as
 * published by the Free Software Foundation, either version 3 of
 * the License, or (at your option) any later version.
 *
 * This program is distributed in the hope that it will be useful,
 * but WITHOUT ANY WARRANTY; without even the implied warranty of
 * MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
 * GNU Affero General Public License for more details.
 * in the LICENSE file at the top-level directory of this distribution.
 * You should have received a copy of the GNU Affero General Public License
 * along with this program.  If not, see <http://www.gnu.org/licenses/>.
 *
 * You can be released from the requirements of the AGPLv3 license by
 * purchasing a commercial license. Please contact licensing@stalw.art
 * for more details.
*/

use std::sync::Arc;

use jmap_proto::{
    error::{method::MethodError, request::RequestError},
    method::{
        get, query,
        set::{self},
    },
    request::{method::MethodName, Call, Request, RequestMethod},
    response::{Response, ResponseMethod},
    types::collection::Collection,
};
use utils::listener::ServerInstance;

use crate::{auth::AccessToken, JMAP};

impl JMAP {
    pub async fn handle_request(
        &self,
        request: Request,
        access_token: Arc<AccessToken>,
        instance: &Arc<ServerInstance>,
    ) -> Result<Response, RequestError> {
        let mut response = Response::new(
            access_token.state(),
            request.created_ids.unwrap_or_default(),
            request.method_calls.len(),
        );
        let add_created_ids = !response.created_ids.is_empty();

        for mut call in request.method_calls {
            // Resolve result and id references
            if let Err(method_error) = response.resolve_references(&mut call.method) {
                response.push_response(call.id, MethodName::error(), method_error);
                continue;
            }

            loop {
                let mut next_call = None;

                // Add response
                match self
                    .handle_method_call(call.method, &access_token, &mut next_call, instance)
                    .await
                {
                    Ok(mut method_response) => {
                        match &mut method_response {
                            ResponseMethod::Set(set_response) => {
                                // Add created ids
                                if add_created_ids {
                                    set_response.update_created_ids(&mut response);
                                }

                                // Publish state changes
                                if let Some(state_change) = set_response.state_change.take() {
                                    self.broadcast_state_change(state_change).await;
                                }
                            }
                            ResponseMethod::ImportEmail(import_response) => {
                                // Add created ids
                                if add_created_ids {
                                    import_response.update_created_ids(&mut response);
                                }

                                // Publish state changes
                                if let Some(state_change) = import_response.state_change.take() {
                                    self.broadcast_state_change(state_change).await;
                                }
                            }
                            ResponseMethod::Copy(copy_response) => {
                                // Publish state changes
                                if let Some(state_change) = copy_response.state_change.take() {
                                    self.broadcast_state_change(state_change).await;
                                }
                            }
                            _ => {}
                        }

                        response.push_response(call.id, call.name, method_response);
                    }
                    Err(err) => {
                        response.push_error(call.id, err);
                    }
                }

                // Process next call
                if let Some(next_call) = next_call {
                    call = next_call;
                    call.id = response.method_responses.last().unwrap().id.clone();
                } else {
                    break;
                }
            }
        }

        Ok(response)
    }

    async fn handle_method_call(
        &self,
        method: RequestMethod,
        access_token: &AccessToken,
        next_call: &mut Option<Call<RequestMethod>>,
        instance: &Arc<ServerInstance>,
    ) -> Result<ResponseMethod, MethodError> {
        Ok(match method {
            RequestMethod::Get(mut req) => match req.take_arguments() {
                get::RequestArguments::Email(arguments) => {
                    access_token.assert_has_access(req.account_id, Collection::Email)?;

                    self.email_get(req.with_arguments(arguments), access_token)
                        .await?
                        .into()
                }
                get::RequestArguments::Mailbox => {
                    access_token.assert_has_access(req.account_id, Collection::Mailbox)?;

                    self.mailbox_get(req, access_token).await?.into()
                }
                get::RequestArguments::Thread => {
                    access_token.assert_has_access(req.account_id, Collection::Email)?;

                    self.thread_get(req).await?.into()
                }
                get::RequestArguments::Identity => {
                    access_token.assert_is_member(req.account_id)?;

                    self.identity_get(req).await?.into()
                }
                get::RequestArguments::EmailSubmission => {
                    access_token.assert_is_member(req.account_id)?;

                    self.email_submission_get(req).await?.into()
                }
                get::RequestArguments::PushSubscription => {
                    self.push_subscription_get(req, access_token).await?.into()
                }
                get::RequestArguments::SieveScript => {
                    access_token.assert_is_member(req.account_id)?;

                    self.sieve_script_get(req).await?.into()
                }
                get::RequestArguments::VacationResponse => {
                    access_token.assert_is_member(req.account_id)?;

                    self.vacation_response_get(req).await?.into()
                }
                get::RequestArguments::Principal => {
                    if self.config.principal_allow_lookups || access_token.is_super_user() {
                        self.principal_get(req).await?.into()
                    } else {
                        return Err(MethodError::Forbidden(
                            "Principal lookups are disabled".to_string(),
                        ));
                    }
                }
            },
            RequestMethod::Query(mut req) => match req.take_arguments() {
                query::RequestArguments::Email(arguments) => {
                    access_token.assert_has_access(req.account_id, Collection::Email)?;

                    self.email_query(req.with_arguments(arguments), access_token)
                        .await?
                        .into()
                }
                query::RequestArguments::Mailbox(arguments) => {
                    access_token.assert_has_access(req.account_id, Collection::Mailbox)?;

                    self.mailbox_query(req.with_arguments(arguments), access_token)
                        .await?
                        .into()
                }
                query::RequestArguments::EmailSubmission => {
                    access_token.assert_is_member(req.account_id)?;

                    self.email_submission_query(req).await?.into()
                }
                query::RequestArguments::SieveScript => {
                    access_token.assert_is_member(req.account_id)?;

                    self.sieve_script_query(req).await?.into()
                }
                query::RequestArguments::Principal => {
                    if self.config.principal_allow_lookups || access_token.is_super_user() {
                        self.principal_query(req).await?.into()
                    } else {
                        return Err(MethodError::Forbidden(
                            "Principal lookups are disabled".to_string(),
                        ));
                    }
                }
            },
            RequestMethod::Set(mut req) => match req.take_arguments() {
                set::RequestArguments::Email => {
                    access_token.assert_has_access(req.account_id, Collection::Email)?;

                    self.email_set(req, access_token).await?.into()
                }
                set::RequestArguments::Mailbox(arguments) => {
                    access_token.assert_has_access(req.account_id, Collection::Mailbox)?;

                    self.mailbox_set(req.with_arguments(arguments), access_token)
                        .await?
                        .into()
                }
                set::RequestArguments::Identity => {
                    access_token.assert_is_member(req.account_id)?;

                    self.identity_set(req, access_token).await?.into()
                }
                set::RequestArguments::EmailSubmission(arguments) => {
                    access_token.assert_is_member(req.account_id)?;

                    self.email_submission_set(req.with_arguments(arguments), instance, next_call)
                        .await?
                        .into()
                }
                set::RequestArguments::PushSubscription => {
                    self.push_subscription_set(req, access_token).await?.into()
                }
                set::RequestArguments::SieveScript(arguments) => {
                    access_token.assert_is_member(req.account_id)?;

                    self.sieve_script_set(req.with_arguments(arguments), access_token)
                        .await?
                        .into()
                }
                set::RequestArguments::VacationResponse => {
                    access_token.assert_is_member(req.account_id)?;

                    self.vacation_response_set(req).await?.into()
                }
            },
            RequestMethod::Changes(req) => self.changes(req, access_token).await?.into(),
            RequestMethod::Copy(req) => {
                access_token
                    .assert_has_access(req.account_id, Collection::Email)?
                    .assert_has_access(req.from_account_id, Collection::Email)?;

                self.email_copy(req, access_token, next_call).await?.into()
            }
            RequestMethod::CopyBlob(req) => self.blob_copy(req, access_token).await?.into(),
            RequestMethod::ImportEmail(req) => {
                access_token.assert_has_access(req.account_id, Collection::Email)?;

                self.email_import(req, access_token).await?.into()
            }
            RequestMethod::ParseEmail(req) => {
                access_token.assert_has_access(req.account_id, Collection::Email)?;

                self.email_parse(req, access_token).await?.into()
            }
            RequestMethod::QueryChanges(req) => self.query_changes(req, access_token).await?.into(),
            RequestMethod::SearchSnippet(req) => {
                access_token.assert_has_access(req.account_id, Collection::Email)?;

                self.email_search_snippet(req, access_token).await?.into()
            }
            RequestMethod::ValidateScript(req) => {
                access_token.assert_is_member(req.account_id)?;

                self.sieve_script_validate(req, access_token).await?.into()
            }
            RequestMethod::Echo(req) => req.into(),
            RequestMethod::Error(error) => return Err(error),
        })
    }
}
