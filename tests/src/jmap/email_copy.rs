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

use jmap::JMAP;
use jmap_client::{client::Client, mailbox::Role};
use jmap_proto::types::id::Id;

use crate::jmap::mailbox::destroy_all_mailboxes;

pub async fn test(server: Arc<JMAP>, client: &mut Client) {
    println!("Running Email Copy tests...");

    // Create a mailbox on account 1
    let ac1_mailbox_id = client
        .set_default_account_id(Id::new(1).to_string())
        .mailbox_create("Copy Test Ac# 1", None::<String>, Role::None)
        .await
        .unwrap()
        .take_id();

    // Insert a message on account 1
    let ac1_email_id = client
        .email_import(
            concat!(
                "From: bill@example.com\r\n",
                "To: jdoe@example.com\r\n",
                "Subject: TPS Report\r\n",
                "\r\n",
                "I'm going to need those TPS reports ASAP. ",
                "So, if you could do that, that'd be great."
            )
            .as_bytes()
            .to_vec(),
            [&ac1_mailbox_id],
            None::<Vec<&str>>,
            None,
        )
        .await
        .unwrap()
        .take_id();

    // Create a mailbox on account 2
    let ac2_mailbox_id = client
        .set_default_account_id(Id::new(2).to_string())
        .mailbox_create("Copy Test Ac# 2", None::<String>, Role::None)
        .await
        .unwrap()
        .take_id();

    // Copy the email and delete it from the first account
    let mut request = client.build();
    request
        .copy_email(Id::new(1).to_string())
        .on_success_destroy_original(true)
        .create(&ac1_email_id)
        .mailbox_id(&ac2_mailbox_id, true)
        .keyword("$draft", true)
        .received_at(311923920);
    let ac2_email_id = request
        .send()
        .await
        .unwrap()
        .method_response_by_pos(0)
        .unwrap_copy_email()
        .unwrap()
        .created(&ac1_email_id)
        .unwrap()
        .take_id();

    // Check that the email was copied
    let email = client
        .email_get(&ac2_email_id, None::<Vec<_>>)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        email.preview().unwrap(),
        "I'm going to need those TPS reports ASAP. So, if you could do that, that'd be great."
    );
    assert_eq!(email.subject().unwrap(), "TPS Report");
    assert_eq!(email.mailbox_ids(), &[&ac2_mailbox_id]);
    assert_eq!(email.keywords(), &["$draft"]);
    assert_eq!(email.received_at().unwrap(), 311923920);

    // Check that the email was deleted
    assert!(client
        .set_default_account_id(Id::new(1).to_string())
        .email_get(&ac1_email_id, None::<Vec<_>>)
        .await
        .unwrap()
        .is_none());

    // Empty store
    destroy_all_mailboxes(client).await;
    client.set_default_account_id(Id::new(2).to_string());
    destroy_all_mailboxes(client).await;
    server.store.assert_is_empty().await;
}
