import * as assert from "assert";

import { getDevnetApi, getRandomSubstrateKeypair } from "../src/substrate"
import { devnet } from "@polkadot-api/descriptors"
import { TypedApi } from "polkadot-api";
import { convertPublicKeyToSs58 } from "../src/address-utils"
import { generateRandomEthersWallet } from "../src/utils";
import { ISubnetABI, ISUBNET_ADDRESS } from "../src/contracts/subnet"
import { ethers } from "ethers"
import { forceSetBalanceToEthAddress, forceSetBalanceToSs58Address } from "../src/subtensor"

describe("Test the Subnet precompile contract", () => {
    // init eth part
    const wallet = generateRandomEthersWallet();
    // init substrate part

    const hotkey1 = getRandomSubstrateKeypair();
    const hotkey2 = getRandomSubstrateKeypair();
    let api: TypedApi<typeof devnet>

    before(async () => {
        // init variables got from await and async
        api = await getDevnetApi()

        await forceSetBalanceToSs58Address(api, convertPublicKeyToSs58(hotkey1.publicKey))
        await forceSetBalanceToSs58Address(api, convertPublicKeyToSs58(hotkey2.publicKey))
        await forceSetBalanceToEthAddress(api, wallet.address)
    })

    it("Can register network without identity info", async () => {
        const totalNetwork = await api.query.SubtensorModule.TotalNetworks.getValue()

        const contract = new ethers.Contract(ISUBNET_ADDRESS, ISubnetABI, wallet);
        const tx = await contract.registerNetwork(hotkey1.publicKey);
        await tx.wait();

        const totalNetworkAfterAdd = await api.query.SubtensorModule.TotalNetworks.getValue()
        assert.ok(totalNetwork + 1 === totalNetworkAfterAdd)
    });

    it("Can register network with identity info", async () => {
        const totalNetwork = await api.query.SubtensorModule.TotalNetworks.getValue()

        const contract = new ethers.Contract(ISUBNET_ADDRESS, ISubnetABI, wallet);
        const tx = await contract.registerNetwork(hotkey2.publicKey,
            "name",
            "repo",
            "contact",
            "subnetUrl",
            "discord",
            "description",
            "additional"
        );
        await tx.wait();

        const totalNetworkAfterAdd = await api.query.SubtensorModule.TotalNetworks.getValue()
        assert.ok(totalNetwork + 1 === totalNetworkAfterAdd)
    });
 
    it.only("Can register network with identity info and logo url", async () => {
        const totalNetwork = await api.query.SubtensorModule.TotalNetworks.getValue()

        const contract = new ethers.Contract(ISUBNET_ADDRESS, ISubnetABI, wallet);
        const tx = await contract["registerNetwork(bytes32,string,string,string,string,string,string,string,string)"](
            hotkey2.publicKey,
            "name",
            "repo",
            "contact",
            "subnetUrl",
            "discord",
            "description",
            "logoUrl",
            "additional"
        );
        await tx.wait();

        const totalNetworkAfterAdd = await api.query.SubtensorModule.TotalNetworks.getValue()
        assert.ok(totalNetwork + 1 === totalNetworkAfterAdd)
    });

    it("Can set servingRateLimit parameter", async () => {

        const totalNetwork = await api.query.SubtensorModule.TotalNetworks.getValue()
        const contract = new ethers.Contract(ISUBNET_ADDRESS, ISubnetABI, wallet);
        const netuid = totalNetwork - 1;

        const newValue = 100;
        const tx = await contract.setServingRateLimit(netuid, newValue);
        await tx.wait();

        let onchainValue = await api.query.SubtensorModule.ServingRateLimit.getValue(netuid)


        let valueFromContract = Number(
            await contract.getServingRateLimit(netuid)
        );

        assert.equal(valueFromContract, newValue)
        assert.equal(valueFromContract, onchainValue);
    })


    // minDifficulty hyperparameter
    //
    // disabled: only by sudo
    //
    // newValue = 101;
    // tx = await contract.setMinDifficulty(netuid, newValue);
    // await tx.wait();

    // await usingApi(async (api) => {
    //   onchainValue = Number(
    //     await api.query.subtensorModule.minDifficulty(netuid)
    //   );
    // });

    // valueFromContract = Number(await contract.getMinDifficulty(netuid));

    // expect(valueFromContract).to.eq(newValue);
    // expect(valueFromContract).to.eq(onchainValue);

    it("Can set maxDifficulty parameter", async () => {

        const totalNetwork = await api.query.SubtensorModule.TotalNetworks.getValue()
        const contract = new ethers.Contract(ISUBNET_ADDRESS, ISubnetABI, wallet);
        const netuid = totalNetwork - 1;

        const newValue = 102;
        const tx = await contract.setMaxDifficulty(netuid, newValue);
        await tx.wait();

        let onchainValue = await api.query.SubtensorModule.MaxDifficulty.getValue(netuid)


        let valueFromContract = Number(
            await contract.getMaxDifficulty(netuid)
        );

        assert.equal(valueFromContract, newValue)
        assert.equal(valueFromContract, onchainValue);
    })


    it("Can set weightsVersionKey parameter", async () => {

        const totalNetwork = await api.query.SubtensorModule.TotalNetworks.getValue()
        const contract = new ethers.Contract(ISUBNET_ADDRESS, ISubnetABI, wallet);
        const netuid = totalNetwork - 1;

        const newValue = 103;
        const tx = await contract.setWeightsVersionKey(netuid, newValue);
        await tx.wait();

        let onchainValue = await api.query.SubtensorModule.WeightsVersionKey.getValue(netuid)


        let valueFromContract = Number(
            await contract.getWeightsVersionKey(netuid)
        );

        assert.equal(valueFromContract, newValue)
        assert.equal(valueFromContract, onchainValue);
    })

    // need sudo as origin now
    // it("Can set weightsSetRateLimit parameter", async () => {

    //     const totalNetwork = await api.query.SubtensorModule.TotalNetworks.getValue()
    //     const contract = new ethers.Contract(ISUBNET_ADDRESS, ISubnetABI, wallet);
    //     const netuid = totalNetwork - 1;

    //     const newValue = 104;
    //     const tx = await contract.setWeightsSetRateLimit(netuid, newValue);
    //     await tx.wait();

    //     let onchainValue = await api.query.SubtensorModule.WeightsSetRateLimit.getValue(netuid)


    //     let valueFromContract = Number(
    //         await contract.getWeightsSetRateLimit(netuid)
    //     );

    //     assert.equal(valueFromContract, newValue)
    //     assert.equal(valueFromContract, onchainValue);
    // })

    it("Can set adjustmentAlpha parameter", async () => {

        const totalNetwork = await api.query.SubtensorModule.TotalNetworks.getValue()
        const contract = new ethers.Contract(ISUBNET_ADDRESS, ISubnetABI, wallet);
        const netuid = totalNetwork - 1;

        const newValue = 105;
        const tx = await contract.setAdjustmentAlpha(netuid, newValue);
        await tx.wait();

        let onchainValue = await api.query.SubtensorModule.AdjustmentAlpha.getValue(netuid)


        let valueFromContract = Number(
            await contract.getAdjustmentAlpha(netuid)
        );

        assert.equal(valueFromContract, newValue)
        assert.equal(valueFromContract, onchainValue);
    })

    it("Can set maxWeightLimit parameter", async () => {

        const totalNetwork = await api.query.SubtensorModule.TotalNetworks.getValue()
        const contract = new ethers.Contract(ISUBNET_ADDRESS, ISubnetABI, wallet);
        const netuid = totalNetwork - 1;

        const newValue = 106;
        const tx = await contract.setMaxWeightLimit(netuid, newValue);
        await tx.wait();

        let onchainValue = await api.query.SubtensorModule.MaxWeightsLimit.getValue(netuid)


        let valueFromContract = Number(
            await contract.getMaxWeightLimit(netuid)
        );

        assert.equal(valueFromContract, newValue)
        assert.equal(valueFromContract, onchainValue);
    })

    it("Can set immunityPeriod parameter", async () => {

        const totalNetwork = await api.query.SubtensorModule.TotalNetworks.getValue()
        const contract = new ethers.Contract(ISUBNET_ADDRESS, ISubnetABI, wallet);
        const netuid = totalNetwork - 1;

        const newValue = 107;
        const tx = await contract.setImmunityPeriod(netuid, newValue);
        await tx.wait();

        let onchainValue = await api.query.SubtensorModule.ImmunityPeriod.getValue(netuid)


        let valueFromContract = Number(
            await contract.getImmunityPeriod(netuid)
        );

        assert.equal(valueFromContract, newValue)
        assert.equal(valueFromContract, onchainValue);
    })

    it("Can set minAllowedWeights parameter", async () => {

        const totalNetwork = await api.query.SubtensorModule.TotalNetworks.getValue()
        const contract = new ethers.Contract(ISUBNET_ADDRESS, ISubnetABI, wallet);
        const netuid = totalNetwork - 1;

        const newValue = 108;
        const tx = await contract.setMinAllowedWeights(netuid, newValue);
        await tx.wait();

        let onchainValue = await api.query.SubtensorModule.MinAllowedWeights.getValue(netuid)


        let valueFromContract = Number(
            await contract.getMinAllowedWeights(netuid)
        );

        assert.equal(valueFromContract, newValue)
        assert.equal(valueFromContract, onchainValue);
    })

    it("Can set kappa parameter", async () => {

        const totalNetwork = await api.query.SubtensorModule.TotalNetworks.getValue()
        const contract = new ethers.Contract(ISUBNET_ADDRESS, ISubnetABI, wallet);
        const netuid = totalNetwork - 1;

        const newValue = 109;
        const tx = await contract.setKappa(netuid, newValue);
        await tx.wait();

        let onchainValue = await api.query.SubtensorModule.Kappa.getValue(netuid)


        let valueFromContract = Number(
            await contract.getKappa(netuid)
        );

        assert.equal(valueFromContract, newValue)
        assert.equal(valueFromContract, onchainValue);
    })

    it("Can set rho parameter", async () => {

        const totalNetwork = await api.query.SubtensorModule.TotalNetworks.getValue()
        const contract = new ethers.Contract(ISUBNET_ADDRESS, ISubnetABI, wallet);
        const netuid = totalNetwork - 1;

        const newValue = 110;
        const tx = await contract.setRho(netuid, newValue);
        await tx.wait();

        let onchainValue = await api.query.SubtensorModule.Rho.getValue(netuid)


        let valueFromContract = Number(
            await contract.getRho(netuid)
        );

        assert.equal(valueFromContract, newValue)
        assert.equal(valueFromContract, onchainValue);
    })

    it("Can set activityCutoff parameter", async () => {

        const totalNetwork = await api.query.SubtensorModule.TotalNetworks.getValue()
        const contract = new ethers.Contract(ISUBNET_ADDRESS, ISubnetABI, wallet);
        const netuid = totalNetwork - 1;
        const newValue = await api.query.SubtensorModule.MinActivityCutoff.getValue() + 1;
        const tx = await contract.setActivityCutoff(netuid, newValue);
        await tx.wait();

        let onchainValue = await api.query.SubtensorModule.ActivityCutoff.getValue(netuid)


        let valueFromContract = Number(
            await contract.getActivityCutoff(netuid)
        );

        assert.equal(valueFromContract, newValue)
        assert.equal(valueFromContract, onchainValue);
    })

    it("Can set networkRegistrationAllowed parameter", async () => {

        const totalNetwork = await api.query.SubtensorModule.TotalNetworks.getValue()
        const contract = new ethers.Contract(ISUBNET_ADDRESS, ISubnetABI, wallet);
        const netuid = totalNetwork - 1;

        const newValue = true;
        const tx = await contract.setNetworkRegistrationAllowed(netuid, newValue);
        await tx.wait();

        let onchainValue = await api.query.SubtensorModule.NetworkRegistrationAllowed.getValue(netuid)


        let valueFromContract = Boolean(
            await contract.getNetworkRegistrationAllowed(netuid)
        );

        assert.equal(valueFromContract, newValue)
        assert.equal(valueFromContract, onchainValue);
    })

    it("Can set networkPowRegistrationAllowed parameter", async () => {

        const totalNetwork = await api.query.SubtensorModule.TotalNetworks.getValue()
        const contract = new ethers.Contract(ISUBNET_ADDRESS, ISubnetABI, wallet);
        const netuid = totalNetwork - 1;

        const newValue = true;
        const tx = await contract.setNetworkPowRegistrationAllowed(netuid, newValue);
        await tx.wait();

        let onchainValue = await api.query.SubtensorModule.NetworkPowRegistrationAllowed.getValue(netuid)


        let valueFromContract = Boolean(
            await contract.getNetworkPowRegistrationAllowed(netuid)
        );

        assert.equal(valueFromContract, newValue)
        assert.equal(valueFromContract, onchainValue);
    })

    // minBurn hyperparameter. only sudo can set it now
    // newValue = 112;

    // tx = await contract.setMinBurn(netuid, newValue);
    // await tx.wait();

    // await usingApi(async (api) => {
    //   onchainValue = Number(
    //     await api.query.subtensorModule.minBurn(netuid)
    //   );
    // });

    // valueFromContract = Number(await contract.getMinBurn(netuid));

    // expect(valueFromContract).to.eq(newValue);
    // expect(valueFromContract).to.eq(onchainValue);

    // maxBurn hyperparameter. only sudo can set it now
    // it("Can set maxBurn parameter", async () => {

    //     const totalNetwork = await api.query.SubtensorModule.TotalNetworks.getValue()
    //     const contract = new ethers.Contract(ISUBNET_ADDRESS, ISubnetABI, wallet);
    //     const netuid = totalNetwork - 1;

    //     const newValue = 113;
    //     const tx = await contract.setMaxBurn(netuid, newValue);
    //     await tx.wait();

    //     let onchainValue = await api.query.SubtensorModule.MaxBurn.getValue(netuid)


    //     let valueFromContract = Number(
    //         await contract.getMaxBurn(netuid)
    //     );

    //     assert.equal(valueFromContract, newValue)
    //     assert.equal(valueFromContract, onchainValue);
    // })


    // difficulty hyperparameter (disabled: sudo only)
    // newValue = 114;

    // tx = await contract.setDifficulty(netuid, newValue);
    // await tx.wait();

    // await usingApi(async (api) => {
    //   onchainValue = Number(
    //     await api.query.subtensorModule.difficulty(netuid)
    //   );
    // });

    // valueFromContract = Number(await contract.getDifficulty(netuid));

    // expect(valueFromContract).to.eq(newValue);
    // expect(valueFromContract).to.eq(onchainValue);

    it("Can set bondsMovingAverage parameter", async () => {

        const totalNetwork = await api.query.SubtensorModule.TotalNetworks.getValue()
        const contract = new ethers.Contract(ISUBNET_ADDRESS, ISubnetABI, wallet);
        const netuid = totalNetwork - 1;

        const newValue = 115;
        const tx = await contract.setBondsMovingAverage(netuid, newValue);
        await tx.wait();

        let onchainValue = await api.query.SubtensorModule.BondsMovingAverage.getValue(netuid)


        let valueFromContract = Number(
            await contract.getBondsMovingAverage(netuid)
        );

        assert.equal(valueFromContract, newValue)
        assert.equal(valueFromContract, onchainValue);
    })

    it("Can set commitRevealWeightsEnabled parameter", async () => {

        const totalNetwork = await api.query.SubtensorModule.TotalNetworks.getValue()
        const contract = new ethers.Contract(ISUBNET_ADDRESS, ISubnetABI, wallet);
        const netuid = totalNetwork - 1;

        const newValue = true;
        const tx = await contract.setCommitRevealWeightsEnabled(netuid, newValue);
        await tx.wait();

        let onchainValue = await api.query.SubtensorModule.CommitRevealWeightsEnabled.getValue(netuid)


        let valueFromContract = Boolean(
            await contract.getCommitRevealWeightsEnabled(netuid)
        );

        assert.equal(valueFromContract, newValue)
        assert.equal(valueFromContract, onchainValue);
    })

    it("Can set liquidAlphaEnabled parameter", async () => {

        const totalNetwork = await api.query.SubtensorModule.TotalNetworks.getValue()
        const contract = new ethers.Contract(ISUBNET_ADDRESS, ISubnetABI, wallet);
        const netuid = totalNetwork - 1;

        const newValue = true;
        const tx = await contract.setLiquidAlphaEnabled(netuid, newValue);
        await tx.wait();

        let onchainValue = await api.query.SubtensorModule.LiquidAlphaOn.getValue(netuid)


        let valueFromContract = Boolean(
            await contract.getLiquidAlphaEnabled(netuid)
        );

        assert.equal(valueFromContract, newValue)
        assert.equal(valueFromContract, onchainValue);
    })

    it("Can set yuma3Enabled hyperparameter", async () =>
    {
      const totalNetwork = await api.query.SubtensorModule.TotalNetworks.getValue()
      const contract = new ethers.Contract(ISUBNET_ADDRESS, ISubnetABI, wallet);
      const netuid = totalNetwork - 1;

      const newValue = true;
      const tx = await contract.setYuma3Enabled(netuid, newValue);
      await tx.wait();

      let onchainValue = await api.query.SubtensorModule.Yuma3On.getValue(netuid)

      let valueFromContract = Boolean(
        await contract.getYuma3Enabled(netuid)
      );
      assert.equal(valueFromContract, newValue)
      assert.equal(valueFromContract, onchainValue);
    })


    it("Can set alphaValues parameter", async () => {
        const totalNetwork = await api.query.SubtensorModule.TotalNetworks.getValue()
        const contract = new ethers.Contract(ISUBNET_ADDRESS, ISubnetABI, wallet);
        const netuid = totalNetwork - 1;

        const newValue = [118, 52429];
        const tx = await contract.setAlphaValues(netuid, newValue[0], newValue[1]);
        await tx.wait();

        let onchainValue = await api.query.SubtensorModule.AlphaValues.getValue(netuid)

        let value = await contract.getAlphaValues(netuid)
        let valueFromContract = [Number(value[0]), Number(value[1])]

        assert.equal(valueFromContract[0], newValue[0])
        assert.equal(valueFromContract[1], newValue[1])
        assert.equal(valueFromContract[0], onchainValue[0]);
        assert.equal(valueFromContract[1], onchainValue[1]);
    })

    it("Can set commitRevealWeightsInterval parameter", async () => {
        const totalNetwork = await api.query.SubtensorModule.TotalNetworks.getValue()
        const contract = new ethers.Contract(ISUBNET_ADDRESS, ISubnetABI, wallet);
        const netuid = totalNetwork - 1;

        const newValue = 119;
        const tx = await contract.setCommitRevealWeightsInterval(netuid, newValue);
        await tx.wait();

        let onchainValue = await api.query.SubtensorModule.RevealPeriodEpochs.getValue(netuid)

        let valueFromContract = Number(
            await contract.getCommitRevealWeightsInterval(netuid)
        );

        assert.equal(valueFromContract, newValue)
        assert.equal(valueFromContract, onchainValue);
    })
})
