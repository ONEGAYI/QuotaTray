# Aliyun Balance Monitoring Setup Guide

This guide walks you through setting up **Aliyun account balance** monitoring in QuotaTray (Bailian pay-as-you-go usage is billed against this balance). It takes about 5 minutes and requires an account that can sign in to the [Aliyun console](https://home.aliyun.com).

> Tip: this guide is organized step by step. Each step stands on its own — follow along in order, or jump straight to the step where you got stuck.

## What gets monitored

QuotaTray queries the **account-level balance** (available credit) of your Aliyun account, not a Bailian-specific balance:

- Pay-as-you-go Bailian usage is deducted from it, and your top-ups land here;
- But unsettled charges from other cloud products under the same account (ECS, OSS, etc.) also draw from it;
- In other words, this number is how much your entire Aliyun account can still spend.

## Before you start

Balance queries use an Aliyun **AccessKey** (a pair of access credentials), **not** the `sk-` model key from the Bailian API — model keys cannot query balances.

For safety, we do not use the primary account key. Instead, we create a **dedicated RAM user with "read billing only" permission**. A leaked primary key compromises the whole account, while a leaked read-only user only exposes the balance figure.

## Step 1: Create a RAM user

1. Open the [RAM console - Users page](https://ram.console.aliyun.com/users) and click **Create User**;
2. Pick any login name (for example `quotatray-readonly`); the remark can note it is dedicated to QuotaTray balance monitoring;
3. Under access mode, check **only** `OpenAPI Access` (permanent AccessKey);
4. Do **not** check `Console Access` — this scenario only needs API calls, and enabling console password sign-in widens the leak surface (Aliyun shows the same yellow warning on the page);
5. Check `I confirm to create an AccessKey` at the bottom and click **OK**.

## Step 2: Grant billing read-only permission

1. After creation, open the user's detail page and switch to the **Permissions** tab;
2. Click **Grant Permission**, then search for `AliyunBSSReadOnlyAccess` in the dialog;
3. Check the policy and confirm. It only allows **reading billing information** — no spending, no resource changes.

Grants take effect immediately, no waiting needed.

## Step 3: Create and save the AccessKey

1. Go back to the user's **Authentication** tab, find the **AccessKeys** section, and click **Create AccessKey**;
2. The dialog shows the `AccessKey ID` (starting with `LTAI`) and the `AccessKey Secret`;
3. **Copy and save both values right away** — the Secret is shown only once and cannot be viewed again after the dialog closes; if lost, the only option is to delete and recreate the key.

## Step 4: Add it in QuotaTray

1. Add a new provider in QuotaTray and pick **Aliyun Balance**;
2. Enter the `AccessKey ID` in the credential field and the `AccessKey Secret` in the second credential field;
3. Save and query immediately — the card should show the current available credit and currency (CNY on the China site).

Both values are stored on disk **AES-GCM encrypted** under the master key, which lives in the OS credential store — neither ever appears in plaintext in any config file.

## Security notes

- **Dedicated user + least privilege**: the RAM user created in this guide has billing read-only permission by design; do not switch to the primary account AccessKey just for convenience.
- **Keep the Secret safe**: never paste it into chat tools, cloud drives, blogs, or code repositories.
- **If it leaks**: go to the RAM user's authentication page and **disable or delete** the AccessKey immediately, then create a new one.

## FAQ

- **Query fails with "NotAuthorized / This API is not authorized"**: the Step 2 grant is missing. Go back to the Permissions tab, confirm `AliyunBSSReadOnlyAccess` is checked, save, and retry.
- **Error "InvalidAccessKeyId"**: the ID was copied incompletely or has stray spaces — copy it again. Note that the ID and the Secret must come from the same AccessKey.
- **The balance doesn't match expectations**: check the scope first — this is account-level available credit, and arrears from other cloud products drag it down; cash top-ups may also take a few minutes to arrive.
- **Can I use a Bailian model key** (starting with `sk-`)? No. Model keys can only call models; there is no official API to query balance with them.
- **Lost the Secret?**: it cannot be recovered. Delete that AccessKey, create a new pair, then update the credentials in QuotaTray.
- **International site (alibabacloud.com) accounts**: endpoint and currency behavior are not verified in this project yet — if you hit issues, please open an issue.
