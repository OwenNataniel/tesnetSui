// Copyright (c), Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0
import { useEffect, useState } from 'react';
import {
  useCurrentAccount,
  useSignAndExecuteTransaction,
  useSignPersonalMessage,
  useSuiClient,
} from '@mysten/dapp-kit';
import { useNetworkVariable } from './networkConfig';
import { AlertDialog, Button, Card, Dialog, Flex } from '@radix-ui/themes';
import { coinWithBalance, Transaction } from '@mysten/sui/transactions';
import { fromHex, SUI_CLOCK_OBJECT_ID } from '@mysten/sui/utils';
import { SealClient, SessionKey, getAllowlistedKeyServers } from '@mysten/seal';
import { useParams } from 'react-router-dom';
import { downloadAndDecrypt, getObjectExplorerLink, MoveCallConstructor } from './utils';

const TTL_MIN = 10;
export interface FeedData {
  id: string;
  fee: string;
  ttl: string;
  owner: string;
  name: string;
  blobIds: string[];
  subscriptionId?: string;
}

const FeedsToSubscribe: React.FC<{ suiAddress: string }> = ({ suiAddress }) => {
  const suiClient = useSuiClient();
  const { id } = useParams();

  const client = new SealClient({
    suiClient,
    serverObjectIds: getAllowlistedKeyServers('testnet'),
    verifyKeyServers: false,
  });
  const [feed, setFeed] = useState<FeedData>();
  const [decryptedFileUrls, setDecryptedFileUrls] = useState<string[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [partialError, setPartialError] = useState<string | null>(null);
  const packageId = useNetworkVariable('packageId');
  const currentAccount = useCurrentAccount();
  const [currentSessionKey, setCurrentSessionKey] = useState<SessionKey | null>(null);
  const [reloadKey, setReloadKey] = useState(0);
  const [isDialogOpen, setIsDialogOpen] = useState(false);

  const { mutate: signPersonalMessage } = useSignPersonalMessage();

  const { mutate: signAndExecute } = useSignAndExecuteTransaction({
    execute: async ({ bytes, signature }) =>
      await suiClient.executeTransactionBlock({
        transactionBlock: bytes,
        signature,
        options: {
          showRawEffects: true,
          showEffects: true,
        },
      }),
  });

  useEffect(() => {
    // Memanggil getFeed segera
    getFeed();

    // Menyiapkan interval untuk memanggil getFeed setiap 3 detik
    const intervalId = setInterval(() => {
      getFeed();
    }, 3000);

    // Membersihkan interval saat komponen di-unmount
    return () => clearInterval(intervalId);
  }, [id, suiAddress, packageId, suiClient]);

  async function getFeed() {
    // Mendapatkan semua objek terenkripsi untuk ID layanan yang diberikan
    const encryptedObjects = await suiClient
      .getDynamicFields({
        parentId: id!,
      })
      .then((res) => res.data.map((obj) => obj.name.value as string));

    // Mendapatkan objek layanan saat ini
    const service = await suiClient.getObject({
      id: id!,
      options: { showContent: true },
    });
    const service_fields = (service.data?.content as { fields: any })?.fields || {};

    // Mendapatkan semua langganan untuk alamat sui yang diberikan
    const res = await suiClient.getOwnedObjects({
      owner: suiAddress,
      options: {
        showContent: true,
        showType: true,
      },
      filter: {
        StructType: `${packageId}::subscription::Subscription`,
      },
    });

    // Mendapatkan timestamp saat ini
    const clock = await suiClient.getObject({
      id: '0x6',
      options: { showContent: true },
    });
    const fields = (clock.data?.content as { fields: any })?.fields || {};
    const current_ms = fields.timestamp_ms;

    // Menemukan langganan yang kedaluwarsa untuk layanan yang diberikan jika ada
    const valid_subscription = res.data
      .map((obj) => {
        const fields = (obj!.data!.content as { fields: any }).fields;
        const x = {
          id: fields?.id.id,
          created_at: parseInt(fields?.created_at),
          service_id: fields?.service_id,
        };
        return x;
      })
      .filter((item) => item.service_id === service_fields.id.id)
      .find((item) => {
        return item.created_at + parseInt(service_fields.ttl) > current_ms;
      });

    const feed = {
      ...service_fields,
      id: service_fields.id.id,
      blobIds: encryptedObjects,
      subscriptionId: valid_subscription?.id,
    } as FeedData;
    setFeed(feed);
  }

  function constructMoveCall(
    packageId: string,
    serviceId: string,
    subscriptionId: string,
  ): MoveCallConstructor {
    return (tx: Transaction, id: string) => {
      tx.moveCall({
        target: `${packageId}::subscription::seal_approve`,
        arguments: [
          tx.pure.vector('u8', fromHex(id)),
          tx.object(subscriptionId),
          tx.object(serviceId),
          tx.object(SUI_CLOCK_OBJECT_ID),
        ],
      });
    };
  }

  async function handleSubscribe(serviceId: string, fee: number) {
    const address = currentAccount?.address!;
    const tx = new Transaction();
    tx.setGasBudget(10000000);
    tx.setSender(address);
    const subscription = tx.moveCall({
      target: `${packageId}::subscription::subscribe`,
      arguments: [
        coinWithBalance({
          balance: BigInt(fee),
        }),
        tx.object(serviceId),
        tx.object(SUI_CLOCK_OBJECT_ID),
      ],
    });
    tx.moveCall({
      target: `${packageId}::subscription::transfer`,
      arguments: [tx.object(subscription), tx.pure.address(address)],
    });

    signAndExecute(
      {
        transaction: tx,
      },
      {
        onSuccess: async (result) => {
          console.log('res', result);
          getFeed();
        },
      },
    );
  }

  const onView = async (
    blobIds: string[],
    serviceId: string,
    fee: number,
    subscriptionId?: string,
  ) => {
    if (!subscriptionId) {
      return handleSubscribe(serviceId, fee);
    }

    if (
      currentSessionKey &&
      !currentSessionKey.isExpired() &&
      currentSessionKey.getAddress() === suiAddress
    ) {
      const moveCallConstructor = constructMoveCall(packageId, serviceId, subscriptionId);
      downloadAndDecrypt(
        blobIds,
        currentSessionKey,
        suiClient,
        client,
        moveCallConstructor,
        setError,
        setDecryptedFileUrls,
        setIsDialogOpen,
        setReloadKey,
      );
      return;
    }
    setCurrentSessionKey(null);

    const sessionKey = new SessionKey({
      address: suiAddress,
      packageId,
      ttlMin: TTL_MIN,
    });

    try {
      signPersonalMessage(
        {
          message: sessionKey.getPersonalMessage(),
        },
        {
          onSuccess: async (result) => {
            await sessionKey.setPersonalMessageSignature(result.signature);
            const moveCallConstructor = await constructMoveCall(
              packageId,
              serviceId,
              subscriptionId,
            );
            await downloadAndDecrypt(
              blobIds,
              sessionKey,
              suiClient,
              client,
              moveCallConstructor,
              setError,
              setDecryptedFileUrls,
              setIsDialogOpen,
              setReloadKey,
            );
            setCurrentSessionKey(sessionKey);
          },
        },
      );
    } catch (error: any) {
      console.error('Error:', error);
    }
  };

  return (
    <Card>
      {feed === undefined ? (
        <p>Menunggu file...</p>
      ) : (
        <Card key={feed!.id}>
          <h2 style={{ marginBottom: '1rem' }}>
            File untuk layanan langganan {feed!.name} (ID {getObjectExplorerLink(feed!.id)})
          </h2>
          <Flex direction="column" gap="2">
            {feed!.blobIds.length === 0 ? (
              <p>Tidak ada file.</p>
            ) : (
              <div>
                <p>{feed!.blobIds.length} file ditemukan. </p>
                <Dialog.Root open={isDialogOpen} onOpenChange={setIsDialogOpen}>
                  <div style={{ display: 'flex', justifyContent: 'flex-start' }}>
                    <Dialog.Trigger>
                      <Button
                        onClick={() =>
                          onView(feed!.blobIds, feed!.id, Number(feed!.fee), feed!.subscriptionId)
                        }
                      >
                        {feed!.subscriptionId ? (
                          <div>Unduh dan Dekripsi Semua File</div>
                        ) : (
                          <div>
                            Berlangganan untuk {feed!.fee} MIST selama{' '}
                            {Math.floor(parseInt(feed!.ttl) / 60 / 1000)} menit
                          </div>
                        )}
                      </Button>
                    </Dialog.Trigger>
                  </div>
                  {decryptedFileUrls.length > 0 && (
                    <Dialog.Content maxWidth="450px" key={reloadKey}>
                      <Dialog.Title>Melihat semua file yang diambil dari Walrus</Dialog.Title>
                      <Flex direction="column" gap="2">
                        {partialError && <p>{partialError}</p>}
                        {decryptedFileUrls.map((decryptedFileUrl, index) => (
                          <div key={index}>
                            <img src={decryptedFileUrl} alt={`Gambar terdekripsi ${index + 1}`} />
                          </div>
                        ))}
                      </Flex>
                      <Flex gap="3" mt="4" justify="end">
                        <Dialog.Close>
                          <Button
                            variant="soft"
                            color="gray"
                            onClick={() => setDecryptedFileUrls([])}
                          >
                            Tutup
                          </Button>
                        </Dialog.Close>
                      </Flex>
                    </Dialog.Content>
                  )}
                </Dialog.Root>
              </div>
            )}
          </Flex>
        </Card>
      )}
      <AlertDialog.Root open={!!error} onOpenChange={() => setError(null)}>
        <AlertDialog.Content maxWidth="450px">
          <AlertDialog.Title>Kesalahan</AlertDialog.Title>
          <AlertDialog.Description size="2">{error}</AlertDialog.Description>

          <Flex gap="3" mt="4" justify="end">
            <AlertDialog.Action>
              <Button variant="solid" color="gray" onClick={() => setError(null)}>
                Tutup
              </Button>
            </AlertDialog.Action>
          </Flex>
        </AlertDialog.Content>
      </AlertDialog.Root>
    </Card>
  );
};

export default FeedsToSubscribe;
